use anyhow::Result;
use serde::Serialize;

use crate::{
    compose::{
        self, CliDockerInspect, ComposeDefaults, ComposeService, ComposeTarget, DiagnosticSeverity,
        ProcessRunner,
    },
    runtime::RuntimeCore,
    setup::SetupStatus,
};

#[derive(Debug, Clone, Serialize)]
pub struct BinaryDoctorReport {
    current_exe: String,
    path_syslog: Option<String>,
    repo_version: String,
    container_version: Option<String>,
    runtime_current: Option<bool>,
    runtime_current_error: Option<String>,
}

impl BinaryDoctorReport {
    pub fn collect() -> Self {
        let current_exe = std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| format!("unknown: {error}"));
        let path_syslog = command_stdout("sh", &["-c", "command -v syslog"]);
        let container_version =
            command_stdout("docker", &["exec", "syslog-mcp", "syslog", "--version"]);
        let (runtime_current, runtime_current_error) = runtime_current_status();
        Self {
            current_exe,
            path_syslog,
            repo_version: env!("CARGO_PKG_VERSION").to_string(),
            container_version,
            runtime_current,
            runtime_current_error,
        }
    }

    fn runtime_error_count(&self) -> u64 {
        if self.runtime_current == Some(false) {
            1
        } else {
            0
        }
    }

    pub fn render_text(&self) {
        println!("current_exe: {}", self.current_exe);
        println!(
            "path_syslog: {}",
            self.path_syslog.as_deref().unwrap_or("-")
        );
        println!("repo_version: {}", self.repo_version);
        println!(
            "container_version: {}",
            self.container_version.as_deref().unwrap_or("-")
        );
        println!(
            "runtime_current: {}",
            self.runtime_current
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        );
        if let Some(error) = &self.runtime_current_error {
            println!("runtime_current_error: {}", error);
        }
    }
}

pub async fn run_binary_doctor(json: bool) -> Result<()> {
    let report = BinaryDoctorReport::collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        report.render_text();
    }
    if report.runtime_current == Some(false) {
        anyhow::bail!("running syslog container is not current");
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct DoctorPhase {
    status: SetupStatus,
    name: String,
    detail: String,
}

impl DoctorPhase {
    fn new(status: SetupStatus, name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status,
            name: name.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct DoctorSection {
    header: &'static str,
    phases: Vec<DoctorPhase>,
}

impl DoctorSection {
    fn new(header: &'static str, phases: Vec<DoctorPhase>) -> Self {
        Self { header, phases }
    }

    fn error_count(&self) -> usize {
        self.phases
            .iter()
            .filter(|phase| matches!(phase.status, SetupStatus::Error))
            .count()
    }

    fn warning_count(&self) -> usize {
        self.phases
            .iter()
            .filter(|phase| matches!(phase.status, SetupStatus::Warn))
            .count()
    }

    fn passed_count(&self) -> usize {
        self.phases
            .iter()
            .filter(|phase| matches!(phase.status, SetupStatus::Ok | SetupStatus::Skipped))
            .count()
    }

    fn render_text(&self) -> usize {
        let errors = self.error_count();
        let warnings = self.warning_count();
        let passed = self.passed_count();
        let counts = match (passed, errors, warnings) {
            (_, 0, 0) => format!("{passed} passed"),
            (0, e, 0) => format!("{e} error"),
            (0, 0, w) => format!("{w} warning"),
            (0, e, w) => format!("{e} error, {w} warning"),
            (_, e, 0) => format!("{passed} passed, {e} error"),
            (_, 0, w) => format!("{passed} passed, {w} warning"),
            (_, e, w) => format!("{passed} passed, {e} error, {w} warning"),
        };
        println!("{:<18} {}", self.header, counts);
        for phase in &self.phases {
            if matches!(phase.status, SetupStatus::Ok | SetupStatus::Skipped) {
                continue;
            }
            println!(
                "  {}  {:<26}  {}",
                status_label(&phase.status),
                phase.name,
                first_meaningful_line(&phase.detail)
            );
        }
        errors
    }
}

#[derive(Debug, Clone)]
struct TextDoctorReport {
    sections: Vec<DoctorSection>,
}

impl TextDoctorReport {
    async fn collect() -> Self {
        Self {
            sections: vec![
                collect_setup_section().await,
                collect_compose_section(),
                collect_binary_section(),
                collect_ai_section().await,
            ],
        }
    }

    fn render(self) -> Result<()> {
        let mut total_errors = 0;
        for section in &self.sections {
            total_errors += section.render_text();
        }

        println!();
        if total_errors == 0 {
            println!("All checks passed.");
            Ok(())
        } else {
            anyhow::bail!("{total_errors} error(s) found")
        }
    }
}

#[derive(Debug, Serialize)]
struct JsonDoctorReport {
    setup: serde_json::Value,
    compose: serde_json::Value,
    binary: BinaryDoctorReport,
    ai: serde_json::Value,
}

impl JsonDoctorReport {
    async fn collect() -> Self {
        let setup = crate::setup::run_setup_doctor()
            .await
            .map(|r| serde_json::to_value(&r).unwrap_or_default())
            .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}));

        let compose_svc =
            ComposeService::new(CliDockerInspect, ProcessRunner, ComposeDefaults::default());
        let compose = compose_svc
            .status(&ComposeTarget::default())
            .map(|r| serde_json::to_value(&r).unwrap_or_default())
            .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}));

        let ai = match RuntimeCore::load_query_only().await {
            Ok(runtime) => runtime
                .service()
                .ai_doctor()
                .await
                .map(|r| serde_json::to_value(&r).unwrap_or_default())
                .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()})),
            Err(e) => serde_json::json!({"error": e.to_string()}),
        };

        Self {
            setup,
            compose,
            binary: BinaryDoctorReport::collect(),
            ai,
        }
    }

    fn error_count(&self) -> u64 {
        let setup_errors = self
            .setup
            .get("blocking_errors")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let setup_dev_errors = ["debug-wrapper-content", "debug-compose-content"]
            .iter()
            .filter(|name| {
                self.setup
                    .get("phases")
                    .and_then(|p| p.as_array())
                    .is_some_and(|phases| {
                        phases.iter().any(|ph| {
                            ph.get("name").and_then(|n| n.as_str()) == Some(name)
                                && matches!(
                                    ph.get("status").and_then(|s| s.as_str()),
                                    Some("error")
                                )
                        })
                    })
            })
            .count() as u64;
        let compose_errors = self
            .compose
            .get("diagnostics")
            .and_then(|v| v.as_array())
            .map(|diagnostics| {
                diagnostics
                    .iter()
                    .filter(|diag| {
                        matches!(
                            diag.get("severity").and_then(|s| s.as_str()),
                            Some("error") | Some("unsafe")
                        )
                    })
                    .count() as u64
            })
            .unwrap_or(0);

        // Counts top-level `{"error": ...}` markers that the collector
        // emits when a section fails wholesale. Per-counter AI failures
        // (checkpoint_error_count / parse_error_count) remain warnings in
        // both renderers — they're inventory signals, not fatal errors,
        // and text-doctor treats them the same way (see render_text in
        // collect_ai_section).
        let top_level_errors = u64::from(self.setup.get("error").is_some())
            + u64::from(self.compose.get("error").is_some())
            + u64::from(self.ai.get("error").is_some());

        setup_errors.saturating_sub(setup_dev_errors)
            + compose_errors
            + self.binary.runtime_error_count()
            + top_level_errors
    }
}

pub async fn run_full_doctor(json: bool) -> Result<()> {
    if json {
        let report = JsonDoctorReport::collect().await;
        let total = report.error_count();
        println!("{}", serde_json::to_string_pretty(&report)?);
        if total > 0 {
            anyhow::bail!("doctor found {total} error(s)");
        }
        return Ok(());
    }

    TextDoctorReport::collect().await.render()
}

async fn collect_setup_section() -> DoctorSection {
    let mut phases = Vec::new();
    let mut seen = std::collections::HashSet::new();
    match crate::setup::run_setup_doctor().await {
        Ok(report) => {
            for phase in &report.phases {
                if phase.name == "runtime-current" || !seen.insert(phase.name.to_string()) {
                    continue;
                }
                let (status, detail) = match phase.name {
                    "debug-wrapper-content" if matches!(phase.status, SetupStatus::Error) => (
                        SetupStatus::Warn,
                        "production binary installed (not the dev wrapper - expected in production)"
                            .to_string(),
                    ),
                    "debug-compose-content" if matches!(phase.status, SetupStatus::Error) => (
                        SetupStatus::Warn,
                        "override uses production config (not the debug build override - expected in production)"
                            .to_string(),
                    ),
                    _ => (phase.status.clone(), phase.detail.clone()),
                };
                phases.push(DoctorPhase::new(status, phase.name.to_string(), detail));
            }
        }
        Err(error) => phases.push(DoctorPhase::new(
            SetupStatus::Error,
            "setup_doctor",
            error.to_string(),
        )),
    }
    DoctorSection::new("Setup", phases)
}

fn collect_compose_section() -> DoctorSection {
    let mut phases = Vec::new();
    let compose_svc =
        ComposeService::new(CliDockerInspect, ProcessRunner, ComposeDefaults::default());
    match compose_svc.status(&ComposeTarget::default()) {
        Ok(status) => {
            let runtime_state = compose::mcp_projection(&status).runtime_state;
            let setup_status = match runtime_state {
                compose::ComposeRuntimeState::Healthy => SetupStatus::Ok,
                compose::ComposeRuntimeState::Degraded => SetupStatus::Warn,
                _ => SetupStatus::Error,
            };
            phases.push(DoctorPhase::new(
                setup_status,
                "status",
                format!(
                    "{} ({})",
                    status.status.as_deref().unwrap_or("unknown"),
                    status.health.as_deref().unwrap_or("no healthcheck")
                ),
            ));
            match status.data_mounts.iter().find(|m| m.target == "/data") {
                Some(m) => {
                    let src = m
                        .source
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    phases.push(DoctorPhase::new(
                        if m.kind == "bind" {
                            SetupStatus::Ok
                        } else {
                            SetupStatus::Error
                        },
                        "data_volume",
                        format!("{} {} -> /data", m.kind, src),
                    ));
                }
                None if matches!(
                    runtime_state,
                    compose::ComposeRuntimeState::Healthy | compose::ComposeRuntimeState::Degraded
                ) =>
                {
                    phases.push(DoctorPhase::new(
                        SetupStatus::Error,
                        "data_volume",
                        "no /data mount",
                    ))
                }
                None => {}
            }
            for diag in &status.diagnostics {
                phases.push(DoctorPhase::new(
                    diag_status(&diag.severity),
                    diag.code.clone(),
                    diag.message.clone(),
                ));
            }
        }
        Err(error) => phases.push(DoctorPhase::new(
            SetupStatus::Error,
            "compose_status",
            error.to_string(),
        )),
    }
    DoctorSection::new("Compose", phases)
}

fn collect_binary_section() -> DoctorSection {
    let binary = BinaryDoctorReport::collect();
    let (status, detail) = match binary.runtime_current {
        Some(true) => (
            SetupStatus::Ok,
            format!(
                "container {} == repo {}",
                binary.container_version.as_deref().unwrap_or("-"),
                binary.repo_version
            ),
        ),
        Some(false) => {
            let detail = if let Some(reason) = binary
                .runtime_current_error
                .as_deref()
                .map(first_meaningful_line)
                .filter(|s| !s.is_empty())
            {
                reason.to_string()
            } else {
                format!(
                    "container {} != repo {} - run: syslog compose up",
                    binary.container_version.as_deref().unwrap_or("-"),
                    binary.repo_version
                )
            };
            (SetupStatus::Error, detail)
        }
        None => (
            SetupStatus::Warn,
            binary
                .runtime_current_error
                .as_deref()
                .map(first_meaningful_line)
                .unwrap_or("could not determine container version")
                .to_string(),
        ),
    };
    DoctorSection::new(
        "Binary",
        vec![DoctorPhase::new(status, "runtime_current", detail)],
    )
}

async fn collect_ai_section() -> DoctorSection {
    let mut phases = Vec::new();
    match RuntimeCore::load_query_only().await {
        Ok(runtime) => match runtime.service().ai_doctor().await {
            Ok(ai) => {
                for (name, root) in [
                    ("claude_root", &ai.claude_root),
                    ("codex_root", &ai.codex_root),
                ] {
                    let (status, detail) = if root.exists && root.readable {
                        (SetupStatus::Ok, root.path.clone())
                    } else if !root.exists {
                        (SetupStatus::Warn, format!("{} (missing)", root.path))
                    } else {
                        (SetupStatus::Warn, format!("{} (not readable)", root.path))
                    };
                    phases.push(DoctorPhase::new(status, name, detail));
                }
                phases.push(DoctorPhase::new(
                    if ai.schema_current {
                        SetupStatus::Ok
                    } else {
                        SetupStatus::Warn
                    },
                    "db_schema",
                    format!(
                        "version {}/{} last_migration={}",
                        ai.db_schema_version,
                        ai.known_schema_version,
                        ai.db_last_migration_at.as_deref().unwrap_or("-")
                    ),
                ));
                phases.push(DoctorPhase::new(
                    if ai.checkpoint_error_count > 0 || ai.missing_checkpoint_count > 0 {
                        SetupStatus::Warn
                    } else {
                        SetupStatus::Ok
                    },
                    "checkpoints",
                    format!(
                        "{} indexed, {} errors, {} missing",
                        ai.checkpoint_count, ai.checkpoint_error_count, ai.missing_checkpoint_count
                    ),
                ));
                if ai.parse_error_count > 0 {
                    phases.push(DoctorPhase::new(
                        SetupStatus::Warn,
                        "parse_errors",
                        format!("{} parse errors", ai.parse_error_count),
                    ));
                }
                // Always collect indexing-health diagnostics — these are the
                // most useful surfaces during a watcher outage when the start
                // timestamp is unavailable (`n/a`) or parsing failed. Only the
                // schema-drift comparison requires a known watcher start time.
                let process_start_time = ai_watcher_process_start_time();
                if process_start_time.is_none() && ai_watcher_is_active() {
                    // Watcher is up but we couldn't read its start time: the
                    // schema-drift comparison silently skips below, so a
                    // wedged-with-stale-schema watcher would otherwise report
                    // healthy. Surface that explicitly.
                    phases.push(DoctorPhase::new(
                        SetupStatus::Warn,
                        "ai_watch_start_unknown",
                        "watcher is active but ExecMainStartTimestamp could not be parsed; schema-drift check skipped",
                    ));
                }
                match runtime
                    .service()
                    .ai_indexing_health(process_start_time.clone())
                    .await
                {
                    Ok(health) => {
                        if let Some(start) = process_start_time.as_deref() {
                            if health.schema_drift_detected {
                                phases.push(DoctorPhase::new(
                                    SetupStatus::Error,
                                    "ai_watch_schema_drift",
                                    format!(
                                        "watcher started {start}; {} migration(s) applied later; fix: systemctl --user restart syslog-ai-watch.service",
                                        health.schema_drift_migrations.len()
                                    ),
                                ));
                            }
                        }
                        if health.recent_failure_count > 0 || health.recent_schema_error_count > 0 {
                            phases.push(DoctorPhase::new(
                                SetupStatus::Warn,
                                "ai_watch_recent_failures",
                                format!(
                                    "{} parse/index failures in last hour, {} schema-like errors, affected_paths={}",
                                    health.recent_failure_count,
                                    health.recent_schema_error_count,
                                    health.affected_paths.len()
                                ),
                            ));
                        }
                    }
                    Err(error) => phases.push(DoctorPhase::new(
                        SetupStatus::Warn,
                        "ai_watch_health",
                        error.to_string(),
                    )),
                }
            }
            Err(error) => phases.push(DoctorPhase::new(
                SetupStatus::Error,
                "ai_doctor",
                error.to_string(),
            )),
        },
        Err(error) => phases.push(DoctorPhase::new(
            SetupStatus::Error,
            "db_connect",
            error.to_string(),
        )),
    }
    DoctorSection::new("AI Transcripts", phases)
}

pub fn ai_watcher_process_start_time() -> Option<String> {
    const SERVICE: &str = "syslog-ai-watch.service";
    // systemd 247+ renders ExecMainStartTimestamp as `@<unix_seconds>` when
    // passed `--timestamp=unix` — locale/TZ-independent. Older systemd
    // ignores the flag and returns the human form, parsed by the fallback.
    if let Some(usec) = systemctl_unix_timestamp(SERVICE) {
        return Some(usec);
    }
    let output = std::process::Command::new("systemctl")
        .arg("--user")
        .args(["show", "-p", "ExecMainStartTimestamp", "--value", SERVICE])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_systemctl_timestamp_utc(String::from_utf8_lossy(&output.stdout).trim())
}

/// Best-effort check: is `syslog-ai-watch.service` loaded and active right
/// now? Used to distinguish "watcher down, start time legitimately n/a" from
/// "watcher running but start-time parsing failed" — the latter is a
/// diagnostic the operator needs to see.
fn ai_watcher_is_active() -> bool {
    let Ok(output) = std::process::Command::new("systemctl")
        .arg("--user")
        .args(["is-active", "syslog-ai-watch.service"])
        .output()
    else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "active"
}

fn systemctl_unix_timestamp(service: &str) -> Option<String> {
    let output = std::process::Command::new("systemctl")
        .arg("--user")
        .args([
            "show",
            "-p",
            "ExecMainStartTimestamp",
            "--value",
            "--timestamp=unix",
            service,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let Some(stripped) = raw.strip_prefix('@') else {
        // Non-empty stdout without `@` prefix means systemd accepted the
        // call but `--timestamp=unix` was a no-op (pre-247 quietly emits the
        // human form). Caller falls back to the legacy parser; log so a
        // diagnosing operator can tell the difference from "unit not found".
        if !raw.is_empty() {
            tracing::debug!(
                service,
                raw = %raw,
                "systemctl --timestamp=unix not supported; using legacy parser"
            );
        }
        return None;
    };
    let secs: i64 = stripped.split('.').next()?.parse().ok()?;
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)?;
    Some(crate::app::time::rfc3339_z(dt))
}

/// Parse the human-readable `ExecMainStartTimestamp` form emitted by older
/// systemd versions, e.g. `Mon 2026-05-20 17:32:11 EDT`. Returns the time as
/// an RFC3339 millis+Z string so downstream comparisons match the format
/// SQLite stores for `applied_at`.
///
/// This is intentionally a fallback — prefer `systemctl --timestamp=unix`
/// when available, since this parser only knows a handful of US timezone
/// abbreviations and may return `None` on other locales.
pub(crate) fn parse_systemctl_timestamp_utc(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "n/a" {
        return None;
    }
    let (prefix, tz) = raw.rsplit_once(' ')?;
    let naive = chrono::NaiveDateTime::parse_from_str(prefix, "%a %Y-%m-%d %H:%M:%S").ok()?;
    let offset_seconds = match tz {
        "UTC" | "GMT" | "Z" => 0,
        "EST" => -5 * 3600,
        "EDT" => -4 * 3600,
        "CST" => -6 * 3600,
        "CDT" => -5 * 3600,
        "MST" => -7 * 3600,
        "MDT" => -6 * 3600,
        "PST" => -8 * 3600,
        "PDT" => -7 * 3600,
        _ => return None,
    };
    let utc = naive - chrono::TimeDelta::seconds(i64::from(offset_seconds));
    let dt = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(utc, chrono::Utc);
    Some(crate::app::time::rfc3339_z(dt))
}

fn status_label(s: &SetupStatus) -> &'static str {
    match s {
        SetupStatus::Ok => "Ok   ",
        SetupStatus::Warn => "Warn ",
        SetupStatus::Error => "Error",
        SetupStatus::Skipped => "Skip ",
    }
}

fn diag_status(sev: &DiagnosticSeverity) -> SetupStatus {
    match sev {
        DiagnosticSeverity::Error | DiagnosticSeverity::Unsafe => SetupStatus::Error,
        DiagnosticSeverity::Warning => SetupStatus::Warn,
        DiagnosticSeverity::Info => SetupStatus::Ok,
    }
}

fn first_meaningful_line(text: &str) -> &str {
    text.lines().find(|l| !l.trim().is_empty()).unwrap_or(text)
}

fn runtime_current_status() -> (Option<bool>, Option<String>) {
    let Some(script) = runtime_current_script_path() else {
        return (
            None,
            Some("scripts/check-runtime-current.sh not found".into()),
        );
    };
    match std::process::Command::new("bash").arg(script).output() {
        Ok(output) if output.status.success() => (Some(true), None),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            (
                Some(false),
                Some(format!("{stdout}{stderr}").trim().to_string()),
            )
        }
        Err(error) => (None, Some(error.to_string())),
    }
}

fn runtime_current_script_path() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("SYSLOG_RUNTIME_CHECK_SCRIPT")
        .map(std::path::PathBuf::from)
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("scripts/check-runtime-current.sh"));
            candidates.push(exe_dir.join("../scripts/check-runtime-current.sh"));
            candidates.push(exe_dir.join("../../scripts/check-runtime-current.sh"));
            candidates.push(exe_dir.join("../../../scripts/check-runtime-current.sh"));
        }
    }
    candidates.push(std::path::PathBuf::from("scripts/check-runtime-current.sh"));

    candidates.into_iter().find(|path| path.exists())
}

fn command_stdout(command: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(command)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;
