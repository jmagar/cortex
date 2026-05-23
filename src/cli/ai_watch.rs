use anyhow::{bail, Result};
use serde::Serialize;
use std::path::PathBuf;
use syslog_mcp::app::SyslogService;
use syslog_mcp::scanner::{AiDoctorReport, AiIndexingHealth};
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AiWatchStatusReport {
    pub(crate) service: String,
    pub(crate) active: Option<String>,
    pub(crate) enabled: Option<String>,
    pub(crate) main_pid: Option<u32>,
    pub(crate) exec_start: Option<String>,
    pub(crate) exec_main_start_timestamp: Option<String>,
    pub(crate) process_start_time: Option<String>,
    pub(crate) db_path: String,
    pub(crate) health: AiIndexingHealth,
    pub(crate) latest_journal: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]

pub(crate) struct AiSmokeWatchReport {
    pub(crate) session_id: String,
    pub(crate) transcript_path: PathBuf,
    pub(crate) ingested: bool,
    pub(crate) pruned_missing_checkpoint: bool,
    pub(crate) missing_checkpoint_count: i64,
}

pub(crate) struct AiSmokeWatchTarget {
    pub(crate) tool: &'static str,
    pub(crate) project: String,
    pub(crate) transcript_path: PathBuf,
    pub(crate) body: String,
}

pub(crate) async fn ai_smoke_watch(service: &SyslogService) -> Result<AiSmokeWatchReport> {
    let doctor = service.ai_doctor().await?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let session_id = format!("syslogsmokewatch{stamp}{}", std::process::id());
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let target = smoke_watch_target(&doctor, &stamp, &session_id, &now)?;
    if let Some(parent) = target.transcript_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target.transcript_path, &target.body)?;
    let canonical_transcript_path = target.transcript_path.canonicalize()?;

    let mut ingested = false;
    for _ in 0..30 {
        let response = service
            .search_sessions(syslog_mcp::app::SearchSessionsRequest {
                query: session_id.clone(),
                project: Some(target.project.clone()),
                tool: Some(target.tool.into()),
                from: None,
                to: None,
                limit: Some(5),
            })
            .await?;
        if response
            .sessions
            .iter()
            .any(|session| session.session_id == session_id)
        {
            ingested = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    if !ingested {
        let _ = std::fs::remove_file(&target.transcript_path);
        bail!("AI watch smoke file was not ingested within 30s");
    }

    std::fs::remove_file(&target.transcript_path)?;
    let canonical_transcript_path = canonical_transcript_path.to_string_lossy().to_string();
    let mut missing_checkpoint_count = i64::MAX;
    let mut pruned_missing_checkpoint = false;
    for _ in 0..30 {
        let result = service.prune_ai_checkpoints(true, false, Some(500)).await?;
        if result
            .paths
            .iter()
            .any(|path| path == &canonical_transcript_path)
        {
            pruned_missing_checkpoint = true;
        }
        let current_doctor = service.ai_doctor().await?;
        missing_checkpoint_count = current_doctor.missing_checkpoint_count;
        if pruned_missing_checkpoint {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    Ok(AiSmokeWatchReport {
        session_id,
        transcript_path: target.transcript_path,
        ingested,
        pruned_missing_checkpoint,
        missing_checkpoint_count,
    })
}

pub(crate) fn smoke_watch_target(
    doctor: &AiDoctorReport,
    stamp: &str,
    session_id: &str,
    now: &str,
) -> Result<AiSmokeWatchTarget> {
    let project = std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/tmp/syslog-smoke-watch".to_string());
    if doctor.claude_root.exists && doctor.claude_root.readable && doctor.claude_root.writable {
        let root = PathBuf::from(&doctor.claude_root.path);
        let transcript_path = root.join(format!("syslog-smoke-watch-{stamp}.jsonl"));
        let body = serde_json::json!({
            "sessionId": session_id,
            "timestamp": now,
            "cwd": project.clone(),
            "content": format!("{session_id} live watcher smoke probe"),
        })
        .to_string()
            + "\n";
        return Ok(AiSmokeWatchTarget {
            tool: "claude",
            project,
            transcript_path,
            body,
        });
    }
    if doctor.codex_root.exists && doctor.codex_root.readable && doctor.codex_root.writable {
        let root = PathBuf::from(&doctor.codex_root.path);
        let transcript_path = root.join(format!("syslog-smoke-watch-{stamp}.jsonl"));
        let body = serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": session_id,
                "cwd": project.clone(),
            },
        })
        .to_string()
            + "\n"
            + &serde_json::json!({
                "type": "response_item",
                "timestamp": now,
                "payload": {
                    "id": session_id,
                    "content": [{
                        "type": "output_text",
                        "text": format!("{session_id} live watcher smoke probe"),
                    }],
                },
            })
            .to_string()
            + "\n";
        return Ok(AiSmokeWatchTarget {
            tool: "codex",
            project,
            transcript_path,
            body,
        });
    }
    bail!("no writable AI transcript root is available for smoke-watch");
}

pub(crate) async fn ai_watch_status(service: &SyslogService) -> Result<AiWatchStatusReport> {
    const SERVICE: &str = "syslog-ai-watch.service";
    let active = systemctl_user_output(&["is-active", SERVICE]).ok();
    let enabled = systemctl_user_output(&["is-enabled", SERVICE]).ok();
    let main_pid = systemctl_user_output(&["show", "-p", "MainPID", "--value", SERVICE])
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid > 0);
    let exec_start = systemctl_user_output(&["show", "-p", "ExecStart", "--value", SERVICE]).ok();
    let exec_main_start_timestamp =
        systemctl_user_output(&["show", "-p", "ExecMainStartTimestamp", "--value", SERVICE]).ok();
    let process_start_time = syslog_mcp::doctor::ai_watcher_process_start_time();
    let doctor = service.ai_doctor().await?;
    let health = service
        .ai_indexing_health(process_start_time.clone())
        .await?;
    let latest_journal = command_output(
        "journalctl",
        &[
            "--user",
            "-u",
            SERVICE,
            "-n",
            "10",
            "--no-pager",
            "--output",
            "short-iso",
        ],
    )
    .map(|raw| raw.lines().map(str::to_string).collect())
    .unwrap_or_default();
    Ok(AiWatchStatusReport {
        service: SERVICE.to_string(),
        active,
        enabled,
        main_pid,
        exec_start,
        exec_main_start_timestamp,
        process_start_time,
        db_path: doctor.db_path,
        health,
        latest_journal,
    })
}

pub(crate) fn systemctl_user_output(args: &[&str]) -> Result<String> {
    let mut command = std::process::Command::new("systemctl");
    command.arg("--user").args(args);
    let output = command.output()?;
    let output =
        if output.status.success() || std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
            output
        } else if systemctl_needs_user_bus_fallback(&output) {
            if let Some((runtime_dir, bus_address)) = inferred_user_bus_env() {
                std::process::Command::new("systemctl")
                    .env("XDG_RUNTIME_DIR", runtime_dir)
                    .env("DBUS_SESSION_BUS_ADDRESS", bus_address)
                    .arg("--user")
                    .args(args)
                    .output()?
            } else {
                output
            }
        } else {
            output
        };
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() || !stdout.is_empty() {
        return Ok(stdout);
    }
    if !output.status.success() {
        bail!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(stdout)
}

fn systemctl_needs_user_bus_fallback(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr.contains("DBUS_SESSION_BUS_ADDRESS") || stderr.contains("user scope bus")
}

fn inferred_user_bus_env() -> Option<(PathBuf, String)> {
    let runtime_dir = PathBuf::from(format!("/run/user/{}", current_uid()));
    let bus = runtime_dir.join("bus");
    bus.exists()
        .then(|| (runtime_dir, format!("unix:path={}", bus.display())))
}

fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn command_output(program: &str, args: &[&str]) -> Result<String> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    if program == "journalctl" && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        if let Some((runtime_dir, bus_address)) = inferred_user_bus_env() {
            command
                .env("XDG_RUNTIME_DIR", runtime_dir)
                .env("DBUS_SESSION_BUS_ADDRESS", bus_address);
        }
    }
    let output = command.output()?;
    if !output.status.success() {
        bail!(
            "{} {} failed: {}",
            program,
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
#[path = "ai_watch_tests.rs"]
mod tests;
