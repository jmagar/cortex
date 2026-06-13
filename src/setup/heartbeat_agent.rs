use std::io::{self, ErrorKind};
use std::path::Path;
use std::time::Instant;

use tracing::warn;

use crate::heartbeat_agent;

use super::firstrun::parse_env;
use super::systemd::{systemctl_user_named_phase, systemctl_user_state};
use super::{
    HeartbeatAgentAction, PhaseTimer, SetupPhase, SetupReport, SetupStatus, check_file_phase,
    host_local_report_input, setup_path_value, setup_report, write_private_file,
};

const UNIT_NAME: &str = "cortex-heartbeat-agent.service";

pub async fn run_heartbeat_agent_setup(action: HeartbeatAgentAction) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = super::cortex_home_dir()?;
    let env_path = home.join("heartbeat-agent.env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = super::user_home_dir()?;
    let unit_dir = user_home.join(".config/systemd/user");
    let unit_path = unit_dir.join(UNIT_NAME);
    let host_id_path = home.join("heartbeat-host-id");
    let mut phases = Vec::new();

    match action {
        HeartbeatAgentAction::Install => {
            let cortex_bin = super::resolve_cortex_binary()?;
            phases.push(write_heartbeat_agent_env(&env_path)?);
            if has_systemd() {
                std::fs::create_dir_all(&unit_dir)?;
                phases.push(write_heartbeat_agent_unit(
                    &unit_path,
                    &cortex_bin,
                    &env_path,
                    &host_id_path,
                )?);
                phases.push(systemctl_user_named_phase(
                    "heartbeat-agent-daemon-reload",
                    &["daemon-reload"],
                ));
                phases.push(systemctl_user_named_phase(
                    "heartbeat-agent-enabled",
                    &["enable", "--now", UNIT_NAME],
                ));
            } else {
                // No systemd (e.g. Unraid) — install as a Docker Compose service.
                let compose_dir = home.join("compose");
                std::fs::create_dir_all(&compose_dir)?;
                phases.push(write_heartbeat_agent_compose(
                    &compose_dir,
                    &cortex_bin,
                    &env_path,
                    &host_id_path,
                )?);
                phases.push(docker_compose_up_phase(&compose_dir));
            }
        }
        HeartbeatAgentAction::Remove => {
            phases.push(systemctl_user_named_phase(
                "heartbeat-agent-disabled",
                &["disable", "--now", UNIT_NAME],
            ));
            phases.push(remove_file_phase("heartbeat-agent-unit", &unit_path)?);
            phases.push(systemctl_user_named_phase(
                "heartbeat-agent-daemon-reload",
                &["daemon-reload"],
            ));
        }
        HeartbeatAgentAction::Check => {
            let cortex_bin = super::resolve_cortex_binary()?;
            phases.push(check_file_phase(
                "heartbeat-agent-env",
                &env_path,
                "run cortex setup heartbeat-agent install",
            ));
            phases.push(check_file_phase(
                "heartbeat-agent-unit",
                &unit_path,
                "run cortex setup heartbeat-agent install",
            ));
            phases.push(check_heartbeat_agent_content(
                &unit_path,
                &cortex_bin,
                &env_path,
                &host_id_path,
            ));
            phases.push(heartbeat_agent_enabled_phase());
            phases.push(heartbeat_agent_active_phase());
        }
    }

    Ok(setup_report(
        host_local_report_input(
            action.as_str(),
            started.elapsed().as_millis(),
            home,
            env_path,
            compose_dir,
            data_dir,
        ),
        phases,
    ))
}

fn write_heartbeat_agent_env(env_path: &Path) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("heartbeat-agent-env");
    if let Some(parent) = env_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let target = std::env::var("CORTEX_HEARTBEAT_TARGET")
        .ok()
        .or_else(|| read_setup_env_value("CORTEX_HEARTBEAT_TARGET"))
        .unwrap_or_else(|| heartbeat_agent::DEFAULT_TARGET.to_string());
    let token = std::env::var("CORTEX_HEARTBEAT_TOKEN")
        .ok()
        .or_else(|| read_setup_env_value("CORTEX_TOKEN"))
        .or_else(|| read_setup_env_value("CORTEX_HEARTBEAT_TOKEN"));
    let docker = std::env::var("CORTEX_AGENT_DOCKER")
        .ok()
        .unwrap_or_else(|| "false".to_string());
    let journald = std::env::var("CORTEX_AGENT_JOURNALD")
        .ok()
        .unwrap_or_else(|| "false".to_string());
    let docker_url = std::env::var("CORTEX_AGENT_DOCKER_URL")
        .ok()
        .unwrap_or_else(|| heartbeat_agent::DEFAULT_DOCKER_URL.to_string());
    let syslog_file = std::env::var("CORTEX_AGENT_SYSLOG_FILE").ok();
    let syslog_target = std::env::var("CORTEX_SYSLOG_TARGET").ok();
    let mut body = format!(
        "CORTEX_HEARTBEAT_TARGET={}\nRUST_LOG=warn\nCORTEX_AGENT_DOCKER={}\nCORTEX_AGENT_DOCKER_URL={}\nCORTEX_AGENT_JOURNALD={}\n",
        shell_safe_value(&target)?,
        shell_safe_value(&docker)?,
        shell_safe_value(&docker_url)?,
        shell_safe_value(&journald)?,
    );
    if let Some(syslog_file) = syslog_file.filter(|value| !value.trim().is_empty()) {
        body.push_str(&format!(
            "CORTEX_AGENT_SYSLOG_FILE={}\n",
            shell_safe_value(&syslog_file)?
        ));
    }
    if let Some(syslog_target) = syslog_target.filter(|value| !value.trim().is_empty()) {
        body.push_str(&format!(
            "CORTEX_SYSLOG_TARGET={}\n",
            shell_safe_value(&syslog_target)?
        ));
    }
    if let Some(token) = token.filter(|value| !value.trim().is_empty()) {
        body.push_str(&format!(
            "CORTEX_HEARTBEAT_TOKEN={}\n",
            shell_safe_value(&token)?
        ));
    }
    write_private_file(env_path, &body)?;
    Ok(timer.finish(SetupStatus::Ok, format!("wrote {}", env_path.display())))
}

fn write_heartbeat_agent_unit(
    unit_path: &Path,
    cortex_bin: &Path,
    env_path: &Path,
    host_id_path: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("heartbeat-agent-unit");
    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        unit_path,
        heartbeat_agent_unit(cortex_bin, env_path, host_id_path)?,
    )?;
    Ok(timer.finish(SetupStatus::Ok, format!("wrote {}", unit_path.display())))
}

fn check_heartbeat_agent_content(
    unit_path: &Path,
    cortex_bin: &Path,
    env_path: &Path,
    host_id_path: &Path,
) -> SetupPhase {
    let timer = PhaseTimer::start("heartbeat-agent-content");
    let expected = match heartbeat_agent_unit(cortex_bin, env_path, host_id_path) {
        Ok(expected) => expected,
        Err(error) => return timer.finish(SetupStatus::Error, error.to_string()),
    };
    match std::fs::read_to_string(unit_path) {
        Ok(current) if current == expected => timer.finish(
            SetupStatus::Ok,
            "heartbeat agent unit matches generated content",
        ),
        Ok(_) => timer.finish(
            SetupStatus::Error,
            format!(
                "{} does not match generated heartbeat agent unit",
                unit_path.display()
            ),
        ),
        Err(error) => timer.finish(SetupStatus::Error, error.to_string()),
    }
}

fn heartbeat_agent_enabled_phase() -> SetupPhase {
    let timer = PhaseTimer::start("heartbeat-agent-enabled");
    match systemctl_user_state("is-enabled", UNIT_NAME).as_deref() {
        Some("enabled") => timer.finish(SetupStatus::Ok, "enabled"),
        Some(state) => timer.finish(SetupStatus::Warn, state),
        None => timer.finish(SetupStatus::Warn, "unknown"),
    }
}

fn heartbeat_agent_active_phase() -> SetupPhase {
    let timer = PhaseTimer::start("heartbeat-agent-active");
    match systemctl_user_state("is-active", UNIT_NAME).as_deref() {
        Some("active") => timer.finish(SetupStatus::Ok, "active"),
        Some(state) => timer.finish(SetupStatus::Warn, state),
        None => timer.finish(SetupStatus::Warn, "unknown"),
    }
}

fn remove_file_phase(name: &'static str, path: &Path) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start(name);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(timer.finish(SetupStatus::Ok, format!("removed {}", path.display()))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(timer.finish(
            SetupStatus::Ok,
            format!("{} already absent", path.display()),
        )),
        Err(error) => Err(error),
    }
}

fn heartbeat_agent_unit(
    cortex_bin: &Path,
    env_path: &Path,
    host_id_path: &Path,
) -> io::Result<String> {
    let read_write_dir = setup_path_value(host_id_path.parent().unwrap_or_else(|| Path::new("/")))?;
    let cortex_bin = setup_path_value(cortex_bin)?;
    let env_path = setup_path_value(env_path)?;
    let host_id_path = setup_path_value(host_id_path)?;
    Ok(format!(
        "[Unit]\nDescription=cortex heartbeat agent\nDocumentation=https://github.com/jmagar/cortex\nAfter=network-online.target\nWants=network-online.target\nStartLimitIntervalSec=300\nStartLimitBurst=5\n\n[Service]\nType=simple\nEnvironmentFile={env_path}\nExecStart={cortex_bin} heartbeat agent --host-id-path {host_id_path}\nRestart=on-failure\nRestartSec=5\nUMask=0077\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nProtectHome=read-only\nReadWritePaths={}\n\n[Install]\nWantedBy=default.target\n",
        read_write_dir
    ))
}

/// Returns true when systemd --user is available on this host.
fn has_systemd() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "--no-pager", "status"])
        .output()
        .map(|o| o.status.code() != Some(127))
        .unwrap_or(false)
}

fn write_heartbeat_agent_compose(
    compose_dir: &Path,
    cortex_bin: &Path,
    env_path: &Path,
    host_id_path: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("heartbeat-agent-compose");
    let compose_path = compose_dir.join("docker-compose.yml");
    let content = heartbeat_agent_compose(cortex_bin, env_path, host_id_path)?;
    std::fs::write(&compose_path, &content)?;
    Ok(timer.finish(SetupStatus::Ok, format!("wrote {}", compose_path.display())))
}

fn docker_compose_up_phase(compose_dir: &Path) -> SetupPhase {
    let timer = PhaseTimer::start("heartbeat-agent-docker-up");
    let result = std::process::Command::new("docker")
        .args(["compose", "up", "-d", "--remove-orphans"])
        .current_dir(compose_dir)
        .output();
    match result {
        Ok(out) if out.status.success() => timer.finish(SetupStatus::Ok, "container started"),
        Ok(out) => timer.finish(
            SetupStatus::Warn,
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .next()
                .unwrap_or("docker compose up failed")
                .to_string(),
        ),
        Err(e) => timer.finish(SetupStatus::Warn, e.to_string()),
    }
}

fn heartbeat_agent_compose(
    cortex_bin: &Path,
    env_path: &Path,
    host_id_path: &Path,
) -> io::Result<String> {
    let data_dir = host_id_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"));
    let cortex_bin = setup_path_value(cortex_bin)?;
    let env_path = setup_path_value(env_path)?;
    let host_id_path = setup_path_value(host_id_path)?;
    let data_dir = setup_path_value(data_dir)?;
    Ok(format!(
        "services:\n  cortex-heartbeat-agent:\n    image: ubuntu:24.04\n    restart: unless-stopped\n    network_mode: host\n    env_file: {env_path}\n    volumes:\n      - {cortex_bin}:/usr/local/bin/cortex:ro\n      - {data_dir}:{data_dir}\n    command:\n      - /usr/local/bin/cortex\n      - heartbeat\n      - agent\n      - --host-id-path\n      - {host_id_path}\n"
    ))
}

fn read_setup_env_value(key: &str) -> Option<String> {
    let path = super::cortex_home_dir().ok()?.join(".env");
    match std::fs::read_to_string(&path) {
        Ok(raw) => parse_env(&raw).remove(key),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            warn!(path = %path.display(), error = %error, "could not read env file for heartbeat setup");
            None
        }
    }
}

fn shell_safe_value(value: &str) -> io::Result<String> {
    if value
        .chars()
        .any(|ch| ch.is_control() || ch == '\n' || ch == '\r')
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "heartbeat environment value contains unsupported characters",
        ));
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_runs_heartbeat_agent_with_private_host_id_path() {
        let unit = heartbeat_agent_unit(
            Path::new("/usr/local/bin/cortex"),
            Path::new("/home/me/.cortex/heartbeat-agent.env"),
            Path::new("/home/me/.cortex/heartbeat-host-id"),
        )
        .unwrap();
        assert!(unit.contains("ExecStart=/usr/local/bin/cortex heartbeat agent --host-id-path /home/me/.cortex/heartbeat-host-id"));
        assert!(unit.contains("EnvironmentFile=/home/me/.cortex/heartbeat-agent.env"));
        assert!(unit.contains("ReadWritePaths=/home/me/.cortex"));
    }

    #[test]
    fn compose_runs_host_binary_with_host_network_and_private_id_path() {
        let compose = heartbeat_agent_compose(
            Path::new("/home/me/.local/bin/cortex"),
            Path::new("/home/me/.cortex/heartbeat-agent.env"),
            Path::new("/home/me/.cortex/heartbeat-host-id"),
        )
        .unwrap();

        assert!(compose.contains("network_mode: host"));
        assert!(compose.contains("env_file: /home/me/.cortex/heartbeat-agent.env"));
        assert!(compose.contains("- /home/me/.local/bin/cortex:/usr/local/bin/cortex:ro"));
        assert!(compose.contains("- /home/me/.cortex:/home/me/.cortex"));
        assert!(compose.contains("- --host-id-path\n      - /home/me/.cortex/heartbeat-host-id"));
    }

    #[test]
    fn shell_safe_value_rejects_control_characters() {
        assert_eq!(shell_safe_value("plain-token").unwrap(), "plain-token");
        assert!(shell_safe_value("bad\nvalue").is_err());
        assert!(shell_safe_value("bad\rvalue").is_err());
    }

    #[test]
    fn remove_file_phase_treats_missing_file_as_success() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.service");

        let phase = remove_file_phase("heartbeat-agent-unit", &missing).unwrap();

        assert!(matches!(phase.status, SetupStatus::Ok));
        assert!(phase.detail.contains("already absent"));
    }

    #[test]
    fn write_heartbeat_agent_env_writes_private_agent_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join("nested/heartbeat-agent.env");

        let phase = write_heartbeat_agent_env(&env_path).unwrap();
        let raw = std::fs::read_to_string(&env_path).unwrap();

        assert!(matches!(phase.status, SetupStatus::Ok));
        assert!(raw.contains(&format!(
            "CORTEX_HEARTBEAT_TARGET={}\n",
            heartbeat_agent::DEFAULT_TARGET
        )));
        assert!(raw.contains("CORTEX_AGENT_DOCKER=false\n"));
        assert!(raw.contains(&format!(
            "CORTEX_AGENT_DOCKER_URL={}\n",
            heartbeat_agent::DEFAULT_DOCKER_URL
        )));
        assert!(raw.contains("CORTEX_AGENT_JOURNALD=false\n"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&env_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn write_heartbeat_agent_unit_creates_parent_and_expected_content() {
        let dir = tempfile::tempdir().unwrap();
        let unit_path = dir
            .path()
            .join(".config/systemd/user/cortex-heartbeat-agent.service");
        let cortex_bin = Path::new("/usr/local/bin/cortex");
        let env_path = Path::new("/home/me/.cortex/heartbeat-agent.env");
        let host_id_path = Path::new("/home/me/.cortex/heartbeat-host-id");

        let phase =
            write_heartbeat_agent_unit(&unit_path, cortex_bin, env_path, host_id_path).unwrap();
        let raw = std::fs::read_to_string(&unit_path).unwrap();

        assert!(matches!(phase.status, SetupStatus::Ok));
        assert!(raw.contains("[Service]\nType=simple"));
        assert!(raw.contains(
            "ExecStart=/usr/local/bin/cortex heartbeat agent --host-id-path /home/me/.cortex/heartbeat-host-id"
        ));
    }

    #[test]
    fn write_heartbeat_agent_compose_creates_compose_file() {
        let dir = tempfile::tempdir().unwrap();
        let compose_dir = dir.path().join("compose");
        std::fs::create_dir_all(&compose_dir).unwrap();

        let phase = write_heartbeat_agent_compose(
            &compose_dir,
            Path::new("/home/me/.local/bin/cortex"),
            Path::new("/home/me/.cortex/heartbeat-agent.env"),
            Path::new("/home/me/.cortex/heartbeat-host-id"),
        )
        .unwrap();
        let raw = std::fs::read_to_string(compose_dir.join("docker-compose.yml")).unwrap();

        assert!(matches!(phase.status, SetupStatus::Ok));
        assert!(raw.contains("cortex-heartbeat-agent:"));
        assert!(raw.contains("restart: unless-stopped"));
        assert!(raw.contains("network_mode: host"));
    }

    #[test]
    fn heartbeat_agent_assets_reject_unit_breaking_paths() {
        assert!(
            heartbeat_agent_unit(
                Path::new("/usr/local/bin/cortex"),
                Path::new("/home/me/bad path/heartbeat-agent.env"),
                Path::new("/home/me/.cortex/heartbeat-host-id"),
            )
            .is_err()
        );
        assert!(
            heartbeat_agent_compose(
                Path::new("/home/me/.local/bin/cortex"),
                Path::new("/home/me/.cortex/heartbeat-agent.env"),
                Path::new("/home/me/bad path/heartbeat-host-id"),
            )
            .is_err()
        );
    }

    #[test]
    fn docker_compose_up_phase_reports_warn_when_workdir_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing-compose-dir");

        let phase = docker_compose_up_phase(&missing);

        assert!(matches!(phase.status, SetupStatus::Warn));
        assert_eq!(phase.name, "heartbeat-agent-docker-up");
        assert!(!phase.detail.trim().is_empty());
    }

    #[test]
    fn content_phase_detects_matching_and_stale_units() {
        let dir = tempfile::tempdir().unwrap();
        let unit_path = dir.path().join("cortex-heartbeat-agent.service");
        let cortex_bin = Path::new("/usr/local/bin/cortex");
        let env_path = Path::new("/home/me/.cortex/heartbeat-agent.env");
        let host_id_path = Path::new("/home/me/.cortex/heartbeat-host-id");
        std::fs::write(
            &unit_path,
            heartbeat_agent_unit(cortex_bin, env_path, host_id_path).unwrap(),
        )
        .unwrap();

        let matching =
            check_heartbeat_agent_content(&unit_path, cortex_bin, env_path, host_id_path);
        assert!(matches!(matching.status, SetupStatus::Ok));

        std::fs::write(&unit_path, "stale unit").unwrap();
        let stale = check_heartbeat_agent_content(&unit_path, cortex_bin, env_path, host_id_path);
        assert!(matches!(stale.status, SetupStatus::Error));
        assert!(
            stale
                .detail
                .contains("does not match generated heartbeat agent unit")
        );
    }
}
