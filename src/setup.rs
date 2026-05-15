use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{self, ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const COMPOSE_ASSET: &str = include_str!("../docker-compose.yml");
const DOCKERFILE_ASSET: &str = include_str!("../config/Dockerfile");

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupMode {
    FirstRun,
    Check,
    Repair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiIndexTimerAction {
    Install,
    Remove,
    Check,
}

impl AiIndexTimerAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "ai-index-timer-install",
            Self::Remove => "ai-index-timer-remove",
            Self::Check => "ai-index-timer-check",
        }
    }
}

impl SetupMode {
    fn mutates(self) -> bool {
        !matches!(self, Self::Check)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::FirstRun => "first-run",
            Self::Check => "check",
            Self::Repair => "repair",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    Ok,
    Warn,
    Error,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupPhase {
    pub name: &'static str,
    pub status: SetupStatus,
    pub detail: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupReport {
    pub mode: &'static str,
    pub elapsed_ms: u128,
    pub home: PathBuf,
    pub env_path: PathBuf,
    pub compose_dir: PathBuf,
    pub data_dir: PathBuf,
    pub health_url: String,
    pub mcp_url: String,
    pub phases: Vec<SetupPhase>,
    pub has_errors: bool,
}

struct PhaseTimer {
    name: &'static str,
    start: Instant,
}

impl PhaseTimer {
    fn start(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }

    fn finish(self, status: SetupStatus, detail: impl Into<String>) -> SetupPhase {
        SetupPhase {
            name: self.name,
            status,
            detail: detail.into(),
            elapsed_ms: self.start.elapsed().as_millis(),
        }
    }
}

pub async fn run_setup(mode: SetupMode) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = syslog_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let mut phases = Vec::new();

    phases.push(filesystem_phase(mode, &home, &data_dir, &compose_dir)?);
    let env = if mode.mutates() {
        let env_result = ensure_env_file(&env_path, &data_dir)?;
        phases.push(env_result.phase);
        phases.push(write_compose_assets(&compose_dir)?);
        Some(env_result.values)
    } else {
        phases.push(check_file_phase("env", &env_path, "run syslog setup"));
        phases.push(check_file_phase(
            "compose-assets",
            &compose_dir.join("docker-compose.yml"),
            "run syslog setup repair",
        ));
        None
    };

    phases.push(command_phase("docker", ["--version"]));
    phases.push(command_phase("docker compose", ["compose", "version"]));
    if mode.mutates() {
        phases.push(cleanup_legacy_systemd());
    }

    let prereq_failed = phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error));
    if mode.mutates() && !prereq_failed {
        ensure_network_phase(&mut phases, env.as_ref());
        phases.push(run_compose_phase(
            &compose_dir,
            &env_path,
            &["pull", "--ignore-buildable"],
        ));
        let up_args: &[&str] = if compose_dir.join("docker-compose.override.yml").exists() {
            &["up", "-d", "--build"]
        } else {
            &["up", "-d"]
        };
        phases.push(run_compose_phase(&compose_dir, &env_path, up_args));
        phases.push(health_phase(&env));
    } else {
        phases.push(SetupPhase {
            name: "compose-up",
            status: SetupStatus::Skipped,
            detail: if prereq_failed {
                "skipped because earlier checks failed".to_string()
            } else {
                "check mode does not start Docker services".to_string()
            },
            elapsed_ms: 0,
        });
    }

    let elapsed_ms = started.elapsed().as_millis();
    let has_errors = phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error));
    let port = env
        .as_ref()
        .and_then(|values| values.get("SYSLOG_MCP_PORT"))
        .cloned()
        .unwrap_or_else(|| "3100".to_string());
    Ok(SetupReport {
        mode: mode.as_str(),
        elapsed_ms,
        home,
        env_path,
        compose_dir,
        data_dir,
        health_url: format!("http://127.0.0.1:{port}/health"),
        mcp_url: format!("http://127.0.0.1:{port}/mcp"),
        phases,
        has_errors,
    })
}

pub async fn run_ai_index_timer_setup(action: AiIndexTimerAction) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = syslog_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = user_home_dir()?;
    let bin_path = user_home.join(".local/bin/syslog-ai-index");
    let systemd_dir = user_home.join(".config/systemd/user");
    let service_path = systemd_dir.join("syslog-ai-index.service");
    let timer_path = systemd_dir.join("syslog-ai-index.timer");
    let mut phases = Vec::new();

    match action {
        AiIndexTimerAction::Install => {
            phases.push(install_ai_index_timer_files(
                &bin_path,
                &systemd_dir,
                &service_path,
                &timer_path,
            )?);
            phases.push(systemctl_user_phase(&["daemon-reload"]));
            phases.push(systemctl_user_phase(&[
                "enable",
                "--now",
                "syslog-ai-index.timer",
            ]));
        }
        AiIndexTimerAction::Remove => {
            phases.push(systemctl_user_phase(&[
                "disable",
                "--now",
                "syslog-ai-index.timer",
            ]));
            phases.push(remove_ai_index_timer_files(
                &bin_path,
                &service_path,
                &timer_path,
            )?);
            phases.push(systemctl_user_phase(&["daemon-reload"]));
        }
        AiIndexTimerAction::Check => {
            phases.push(check_file_phase(
                "ai-index-bin",
                &bin_path,
                "run syslog setup ai-index-timer install",
            ));
            phases.push(check_file_phase(
                "ai-index-service",
                &service_path,
                "run syslog setup ai-index-timer install",
            ));
            phases.push(check_file_phase(
                "ai-index-timer",
                &timer_path,
                "run syslog setup ai-index-timer install",
            ));
            phases.push(systemctl_user_phase(&[
                "is-enabled",
                "syslog-ai-index.timer",
            ]));
        }
    }

    let elapsed_ms = started.elapsed().as_millis();
    let has_errors = phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error));
    Ok(SetupReport {
        mode: action.as_str(),
        elapsed_ms,
        home,
        env_path,
        compose_dir,
        data_dir,
        health_url: "host-local helper".to_string(),
        mcp_url: "host-local helper".to_string(),
        phases,
        has_errors,
    })
}

fn user_home_dir() -> io::Result<PathBuf> {
    let home =
        std::env::var("HOME").map_err(|_| io::Error::new(ErrorKind::NotFound, "HOME is unset"))?;
    Ok(PathBuf::from(home))
}

pub fn syslog_home_dir() -> io::Result<PathBuf> {
    if let Ok(value) = std::env::var("SYSLOG_MCP_HOME") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return validate_absolute_home(PathBuf::from(trimmed));
        }
    }
    let home =
        std::env::var("HOME").map_err(|_| io::Error::new(ErrorKind::NotFound, "HOME is unset"))?;
    validate_absolute_home(PathBuf::from(home).join(".syslog-mcp"))
}

fn validate_absolute_home(path: PathBuf) -> io::Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "setup home must be an absolute path without '..': {}",
                path.display()
            ),
        ));
    }
    Ok(path)
}

fn filesystem_phase(
    mode: SetupMode,
    home: &Path,
    data_dir: &Path,
    compose_dir: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("filesystem");
    if mode.mutates() {
        ensure_private_dir(home)?;
        ensure_private_dir(data_dir)?;
        std::fs::create_dir_all(compose_dir)?;
        return Ok(timer.finish(SetupStatus::Ok, format!("initialized {}", home.display())));
    }
    if home.is_dir() && data_dir.is_dir() && compose_dir.is_dir() {
        Ok(timer.finish(SetupStatus::Ok, format!("found {}", home.display())))
    } else {
        Ok(timer.finish(
            SetupStatus::Warn,
            format!(
                "missing setup dirs under {}; run syslog setup",
                home.display()
            ),
        ))
    }
}

fn ensure_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        std::fs::set_permissions(path, PermissionsExt::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

struct EnvResult {
    phase: SetupPhase,
    values: BTreeMap<String, String>,
}

fn ensure_env_file(path: &Path, data_dir: &Path) -> io::Result<EnvResult> {
    let timer = PhaseTimer::start("env");
    let mut env = if path.exists() {
        parse_env(&std::fs::read_to_string(path)?)
    } else {
        BTreeMap::new()
    };
    let before = env.len();

    insert_process_or_default(&mut env, "SYSLOG_HOST", "0.0.0.0");
    insert_process_or_default(&mut env, "SYSLOG_PORT", "1514");
    insert_process_or_default(&mut env, "SYSLOG_HOST_PORT", "1514");
    insert_process_or_default(&mut env, "SYSLOG_MCP_HOST", "0.0.0.0");
    insert_process_or_default(&mut env, "SYSLOG_MCP_PORT", "3100");
    insert_process_or_default(&mut env, "NO_AUTH", "false");
    insert_process_or_default(&mut env, "SYSLOG_MCP_AUTH_MODE", "bearer");
    insert_process_or_default(&mut env, "SYSLOG_MCP_DB_PATH", "/data/syslog.db");
    insert_process_or_default(
        &mut env,
        "SYSLOG_MCP_DATA_VOLUME",
        &data_dir.display().to_string(),
    );
    insert_process_or_default(&mut env, "SYSLOG_MCP_MAX_DB_SIZE_MB", "8192");
    insert_process_or_default(&mut env, "SYSLOG_MCP_RETENTION_DAYS", "90");
    insert_process_or_default(&mut env, "SYSLOG_BATCH_SIZE", "100");
    insert_process_or_default(&mut env, "SYSLOG_WRITE_CHANNEL_CAPACITY", "10000");
    insert_process_or_default(&mut env, "SYSLOG_DOCKER_INGEST_ENABLED", "false");
    insert_process_or_default(&mut env, "RUST_LOG", "info");
    insert_process_or_default(&mut env, "DOCKER_NETWORK", "syslog-mcp");
    insert_process_optional(&mut env, "SYSLOG_DOCKER_HOSTS");
    insert_process_optional(&mut env, "SYSLOG_MCP_PUBLIC_URL");
    insert_process_optional(&mut env, "SYSLOG_MCP_GOOGLE_CLIENT_ID");
    insert_process_optional(&mut env, "SYSLOG_MCP_GOOGLE_CLIENT_SECRET");
    insert_process_optional(&mut env, "SYSLOG_MCP_AUTH_ADMIN_EMAIL");
    insert_process_optional(&mut env, "SYSLOG_MCP_AUTH_ALLOWED_REDIRECT_URIS");
    insert_process_optional(&mut env, "SYSLOG_MCP_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH");

    let (uid, gid) = current_uid_gid();
    env.entry("SYSLOG_UID".to_string()).or_insert(uid);
    env.entry("SYSLOG_GID".to_string()).or_insert(gid);

    if env
        .get("NO_AUTH")
        .is_none_or(|value| !value.eq_ignore_ascii_case("true"))
        && env
            .get("SYSLOG_MCP_TOKEN")
            .is_none_or(|value| value.trim().is_empty())
    {
        env.insert("SYSLOG_MCP_TOKEN".to_string(), generate_token()?);
    }

    write_env(path, &env)?;
    let added = env.len().saturating_sub(before);
    Ok(EnvResult {
        phase: timer.finish(
            SetupStatus::Ok,
            format!("{} {} keys; added {added}", path.display(), env.len()),
        ),
        values: env,
    })
}

fn insert_process_or_default(env: &mut BTreeMap<String, String>, key: &str, default: &str) {
    if let Some(value) = process_env_value(key) {
        env.insert(key.to_string(), value);
    } else {
        env.entry(key.to_string())
            .or_insert_with(|| default.to_string());
    }
}

fn insert_process_optional(env: &mut BTreeMap<String, String>, key: &str) {
    if let Some(value) = process_env_value(key) {
        env.insert(key.to_string(), value);
    }
}

fn process_env_value(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.contains(['\n', '\r']))
}

fn parse_env(raw: &str) -> BTreeMap<String, String> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn write_env(path: &Path, env: &BTreeMap<String, String>) -> io::Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    let mut out = String::new();
    out.push_str("# syslog-mcp runtime environment.\n");
    out.push_str("# Managed by `syslog setup`; secrets are preserved on repair.\n");
    for (key, value) in env {
        out.push_str(key);
        out.push('=');
        out.push_str(value);
        out.push('\n');
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    options.open(path)?.write_all(out.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn write_compose_assets(compose_dir: &Path) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("compose-assets");
    std::fs::create_dir_all(compose_dir.join("config"))?;
    std::fs::write(
        compose_dir.join("docker-compose.yml"),
        installed_compose_asset(),
    )?;
    std::fs::write(compose_dir.join("config/Dockerfile"), DOCKERFILE_ASSET)?;
    Ok(timer.finish(
        SetupStatus::Ok,
        format!("wrote compose assets under {}", compose_dir.display()),
    ))
}

fn installed_compose_asset() -> String {
    let without_build = COMPOSE_ASSET
        .replace(
            "    # Default to the published image so plugin deploys can `docker compose pull`\n    # without source. Override with --build (or `docker compose build`) for local\n    # source development.\n    image: ghcr.io/jmagar/syslog-mcp:${SYSLOG_MCP_VERSION:-latest}\n    build:\n      context: .\n      dockerfile: config/Dockerfile\n",
            "    image: ghcr.io/jmagar/syslog-mcp:${SYSLOG_MCP_VERSION:-latest}\n",
        );
    assert_ne!(
        without_build, COMPOSE_ASSET,
        "installed compose asset transform failed: expected build stanza was not found"
    );
    let installed = without_build.replace("      - path: .env\n", "      - path: ../.env\n");
    assert_ne!(
        installed, without_build,
        "installed compose asset transform failed: expected env_file path was not found"
    );
    installed
}

fn check_file_phase(name: &'static str, path: &Path, fix: &str) -> SetupPhase {
    let timer = PhaseTimer::start(name);
    if path.exists() {
        timer.finish(SetupStatus::Ok, format!("found {}", path.display()))
    } else {
        timer.finish(
            SetupStatus::Warn,
            format!("missing {}; {fix}", path.display()),
        )
    }
}

fn install_ai_index_timer_files(
    bin_path: &Path,
    systemd_dir: &Path,
    service_path: &Path,
    timer_path: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("ai-index-timer-files");
    if let Some(bin_dir) = bin_path.parent() {
        std::fs::create_dir_all(bin_dir)?;
    }
    std::fs::create_dir_all(systemd_dir)?;
    write_executable_file(bin_path, &ai_index_script())?;
    std::fs::write(service_path, ai_index_service_unit(bin_path))?;
    std::fs::write(timer_path, ai_index_timer_unit())?;
    Ok(timer.finish(
        SetupStatus::Ok,
        format!(
            "wrote {}, {}, {}",
            bin_path.display(),
            service_path.display(),
            timer_path.display()
        ),
    ))
}

fn remove_ai_index_timer_files(
    bin_path: &Path,
    service_path: &Path,
    timer_path: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("ai-index-timer-files");
    for path in [bin_path, service_path, timer_path] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(timer.finish(SetupStatus::Ok, "removed syslog AI index timer files"))
}

fn write_executable_file(path: &Path, body: &str) -> io::Result<()> {
    std::fs::write(path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn ai_index_script() -> String {
    r#"#!/usr/bin/env bash
set -euo pipefail

STATE_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/syslog-mcp"
mkdir -p "$STATE_DIR"
LOCK_FILE="$STATE_DIR/ai-index.lock"
LOG_FILE="$STATE_DIR/ai-index.log"

if [[ -z "${SYSLOG_MCP_DB_PATH:-}" ]]; then
  if [[ -f "${HOME}/.claude/plugins/data/syslog-jmagar-lab/syslog.db" ]]; then
    export SYSLOG_MCP_DB_PATH="${HOME}/.claude/plugins/data/syslog-jmagar-lab/syslog.db"
  else
    export SYSLOG_MCP_DB_PATH="${SYSLOG_MCP_HOME:-${HOME}/.syslog-mcp}/data/syslog.db"
  fi
fi

export SYSLOG_DOCKER_INGEST_ENABLED="${SYSLOG_DOCKER_INGEST_ENABLED:-false}"
export RUST_LOG="${RUST_LOG:-warn}"

{
  printf '== %s ==\n' "$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
  command -v syslog
  syslog --version
  syslog ai index --json
} >>"$LOG_FILE" 2>&1
"#
    .to_string()
}

fn ai_index_service_unit(bin_path: &Path) -> String {
    format!(
        "[Unit]\nDescription=syslog-mcp local AI transcript index\nDocumentation=https://github.com/jmagar/syslog-mcp\n\n[Service]\nType=oneshot\nExecStart={}\n",
        bin_path.display()
    )
}

fn ai_index_timer_unit() -> &'static str {
    "[Unit]\nDescription=Run syslog-mcp local AI transcript index\n\n[Timer]\nOnBootSec=5min\nOnUnitActiveSec=30min\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
}

fn systemctl_user_phase(args: &[&str]) -> SetupPhase {
    let timer = PhaseTimer::start("systemctl-user");
    match run_systemctl_user(args) {
        Ok(output) if output.status.success() => timer.finish(
            SetupStatus::Ok,
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("ok")
                .to_string(),
        ),
        Ok(output) => timer.finish(
            SetupStatus::Warn,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("systemctl --user failed")
                .to_string(),
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            timer.finish(SetupStatus::Warn, "systemctl not found")
        }
        Err(error) => timer.finish(SetupStatus::Warn, error.to_string()),
    }
}

fn run_systemctl_user(args: &[&str]) -> io::Result<std::process::Output> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()?;
    if output.status.success() || std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.contains("DBUS_SESSION_BUS_ADDRESS") && !stderr.contains("user scope bus") {
        return Ok(output);
    }
    let Some((runtime_dir, bus_address)) = inferred_user_bus_env() else {
        return Ok(output);
    };
    Command::new("systemctl")
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("DBUS_SESSION_BUS_ADDRESS", bus_address)
        .arg("--user")
        .args(args)
        .output()
}

fn inferred_user_bus_env() -> Option<(PathBuf, String)> {
    let uid = current_uid_gid().0;
    let runtime_dir = PathBuf::from(format!("/run/user/{uid}"));
    let bus = runtime_dir.join("bus");
    bus.exists()
        .then(|| (runtime_dir, format!("unix:path={}", bus.display())))
}

fn command_phase<const N: usize>(name: &'static str, args: [&str; N]) -> SetupPhase {
    let timer = PhaseTimer::start(name);
    let program = if name == "docker compose" {
        "docker"
    } else {
        name
    };
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => timer.finish(
            SetupStatus::Ok,
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("available")
                .to_string(),
        ),
        Ok(output) => timer.finish(
            SetupStatus::Error,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("command failed")
                .to_string(),
        ),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            timer.finish(SetupStatus::Error, "not found on PATH")
        }
        Err(err) => timer.finish(SetupStatus::Error, err.to_string()),
    }
}

fn cleanup_legacy_systemd() -> SetupPhase {
    let timer = PhaseTimer::start("legacy-systemd");
    let home = match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home),
        Err(_) => {
            return timer.finish(SetupStatus::Skipped, "HOME unset");
        }
    };
    for unit in [
        "syslog-mcp.service",
        "mnemo-index.service",
        "mnemo-index.timer",
    ] {
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", unit])
            .output();
    }
    for name in [
        "syslog-mcp.service",
        "mnemo-index.service",
        "mnemo-index.timer",
    ] {
        let unit = home.join(".config/systemd/user").join(name);
        let dropins = home.join(".config/systemd/user").join(format!("{name}.d"));
        let _ = std::fs::remove_file(&unit);
        let _ = std::fs::remove_dir_all(&dropins);
    }
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    timer.finish(
        SetupStatus::Ok,
        "removed stale syslog-mcp and mnemo-index user units/drop-ins if present",
    )
}

fn ensure_network_phase(phases: &mut Vec<SetupPhase>, env: Option<&BTreeMap<String, String>>) {
    let timer = PhaseTimer::start("docker-network");
    let network = env
        .and_then(|env| env.get("DOCKER_NETWORK"))
        .map(String::as_str)
        .unwrap_or("syslog-mcp");
    let inspect = Command::new("docker")
        .args(["network", "inspect", network])
        .output();
    if inspect.as_ref().is_ok_and(|output| output.status.success()) {
        phases.push(timer.finish(SetupStatus::Ok, format!("{network} exists")));
        return;
    }
    match Command::new("docker")
        .args(["network", "create", network])
        .output()
    {
        Ok(output) if output.status.success() => {
            phases.push(timer.finish(SetupStatus::Ok, format!("created {network}")))
        }
        Ok(output) => phases.push(
            timer.finish(
                SetupStatus::Error,
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("docker network create failed"),
            ),
        ),
        Err(err) => phases.push(timer.finish(SetupStatus::Error, err.to_string())),
    }
}

fn run_compose_phase(compose_dir: &Path, env_path: &Path, args: &[&str]) -> SetupPhase {
    let timer = PhaseTimer::start(if args.first() == Some(&"pull") {
        "compose-pull"
    } else {
        "compose-up"
    });
    let mut command = Command::new("docker");
    command
        .arg("compose")
        .arg("--env-file")
        .arg(env_path)
        .arg("-f")
        .arg(compose_dir.join("docker-compose.yml"));
    let override_path = compose_dir.join("docker-compose.override.yml");
    if override_path.exists() {
        command.arg("-f").arg(override_path);
    }
    command.args(args).current_dir(compose_dir);
    match command.output() {
        Ok(output) if output.status.success() => timer.finish(
            SetupStatus::Ok,
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .last()
                .unwrap_or("ok")
                .to_string(),
        ),
        Ok(output) => timer.finish(
            SetupStatus::Error,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .last()
                .unwrap_or("docker compose failed")
                .to_string(),
        ),
        Err(err) => timer.finish(SetupStatus::Error, err.to_string()),
    }
}

fn health_phase(env: &Option<BTreeMap<String, String>>) -> SetupPhase {
    let timer = PhaseTimer::start("health");
    let port = env
        .as_ref()
        .and_then(|env| env.get("SYSLOG_MCP_PORT"))
        .map(String::as_str)
        .unwrap_or("3100");
    let url = format!("http://127.0.0.1:{port}/health");
    match Command::new("curl")
        .args(["-fsS", "--max-time", "5", &url])
        .output()
    {
        Ok(output) if output.status.success() => {
            timer.finish(SetupStatus::Ok, format!("{url} ready"))
        }
        Ok(output) => timer.finish(
            SetupStatus::Warn,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("health check failed"),
        ),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            timer.finish(SetupStatus::Warn, "curl not found; skipped health check")
        }
        Err(err) => timer.finish(SetupStatus::Warn, err.to_string()),
    }
}

fn current_uid_gid() -> (String, String) {
    let uid = command_stdout("id", ["-u"]).unwrap_or_else(|| "1000".to_string());
    let gid = command_stdout("id", ["-g"]).unwrap_or_else(|| "1000".to_string());
    (uid, gid)
}

fn command_stdout<const N: usize>(program: &str, args: [&str; N]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn generate_token() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    let mut token = String::with_capacity(64);
    for byte in bytes {
        token.push_str(&format!("{byte:02x}"));
    }
    Ok(token)
}
