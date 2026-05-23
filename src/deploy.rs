use std::io::{self, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::Serialize;

use crate::setup::{
    default_env_for_data_dir, dockerfile_asset, installed_compose_asset, render_env, PhaseTimer,
    SetupPhase, SetupStatus,
};

const REMOTE_HOME_SUFFIX: &str = ".syslog-mcp";

#[derive(Debug, Clone, Serialize)]
pub struct RemoteDeployReport {
    pub mode: &'static str,
    pub host: String,
    pub home: String,
    pub env_path: String,
    pub compose_dir: String,
    pub data_dir: String,
    pub health_url: String,
    pub mcp_url: String,
    pub phases: Vec<SetupPhase>,
    pub has_errors: bool,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
struct RemoteOutput {
    status_success: bool,
    stdout: String,
    stderr: String,
}

trait RemoteRunner {
    fn run(&mut self, host: &str, script: &str, stdin: Option<&str>) -> io::Result<RemoteOutput>;
}

struct SshRemoteRunner;

impl RemoteRunner for SshRemoteRunner {
    fn run(&mut self, host: &str, script: &str, stdin: Option<&str>) -> io::Result<RemoteOutput> {
        let mut child = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                host,
                "sh",
                "-s",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        {
            let child_stdin = child.stdin.as_mut().ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "failed to open ssh stdin")
            })?;
            child_stdin.write_all(script.as_bytes())?;
            if let Some(input) = stdin {
                child_stdin.write_all(input.as_bytes())?;
            }
        }

        let output = child.wait_with_output()?;
        Ok(RemoteOutput {
            status_success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

pub fn run_remote_deploy(host: &str, dry_run: bool) -> io::Result<RemoteDeployReport> {
    let mut runner = SshRemoteRunner;
    run_remote_deploy_with_runner(host, dry_run, &mut runner)
}

fn run_remote_deploy_with_runner(
    host: &str,
    dry_run: bool,
    runner: &mut dyn RemoteRunner,
) -> io::Result<RemoteDeployReport> {
    let started = Instant::now();
    let mut phases = Vec::new();

    let ssh_phase = remote_phase(runner, host, "ssh", "true", None)?;
    phases.push(ssh_phase);

    let identity = remote_identity_phase(runner, host)?;
    let identity_values = identity.values;
    phases.push(identity.phase);
    let Some(identity_values) = identity_values else {
        append_skipped(
            &mut phases,
            &[
                "remote-filesystem",
                "remote-env",
                "remote-compose-assets",
                "remote-docker",
                "remote-docker-network",
                "remote-compose-pull",
                "remote-compose-up",
                "remote-health",
            ],
            "skipped because remote identity failed",
        );
        return Ok(report(
            host,
            dry_run,
            "$HOME",
            "3100",
            phases,
            started.elapsed().as_millis(),
        ));
    };
    let remote_home = format!("{}/{}", identity_values.home, REMOTE_HOME_SUFFIX);
    let remote_data_dir = format!("{remote_home}/data");
    let mut env = default_env_for_data_dir(Path::new(&remote_data_dir))?;
    env.insert(
        "SYSLOG_MCP_DATA_VOLUME".to_string(),
        remote_data_dir.clone(),
    );
    env.insert("SYSLOG_UID".to_string(), identity_values.uid.clone());
    env.insert("SYSLOG_GID".to_string(), identity_values.gid.clone());
    let docker_network = env
        .get("DOCKER_NETWORK")
        .cloned()
        .unwrap_or_else(|| "syslog-mcp".to_string());
    let mcp_port = env
        .get("SYSLOG_MCP_PORT")
        .cloned()
        .unwrap_or_else(|| "3100".to_string());

    if dry_run {
        phases.push(remote_phase(
            runner,
            host,
            "remote-filesystem",
            "test -d ~/.syslog-mcp || test -w \"$HOME\"",
            None,
        )?);
        phases.push(remote_phase(
            runner,
            host,
            "remote-docker",
            "docker --version && docker compose version",
            None,
        )?);
        let dry_run_reason = "dry-run does not mutate remote Docker or files";
        phases.push(skip_phase("remote-env", dry_run_reason));
        phases.push(skip_phase("remote-compose-assets", dry_run_reason));
        phases.push(skip_phase(
            "remote-docker-network",
            "dry-run does not create Docker networks",
        ));
        phases.push(skip_phase(
            "remote-compose-pull",
            "dry-run does not pull images",
        ));
        phases.push(skip_phase(
            "remote-compose-up",
            "dry-run does not start Docker services",
        ));
        phases.push(skip_phase(
            "remote-health",
            "dry-run does not check service health",
        ));
        return Ok(report(
            host,
            dry_run,
            &identity_values.home,
            &mcp_port,
            phases,
            started.elapsed().as_millis(),
        ));
    }

    phases.push(remote_phase(
        runner,
        host,
        "remote-filesystem",
        "mkdir -p ~/.syslog-mcp/compose/config ~/.syslog-mcp/data && chmod 700 ~/.syslog-mcp ~/.syslog-mcp/data",
        None,
    )?);
    phases.push(remote_phase(
        runner,
        host,
        "remote-docker",
        "docker --version && docker compose version",
        None,
    )?);
    if phases_have_errors(&phases) {
        append_skipped(
            &mut phases,
            &[
                "remote-env",
                "remote-compose-assets",
                "remote-docker-network",
                "remote-compose-pull",
                "remote-compose-up",
                "remote-health",
            ],
            "skipped because remote prerequisites failed",
        );
        return Ok(report(
            host,
            dry_run,
            &identity_values.home,
            &mcp_port,
            phases,
            started.elapsed().as_millis(),
        ));
    }

    phases.push(write_remote_env_phase(runner, host, &env)?);
    phases.push(write_remote_assets_phase(runner, host)?);
    if phases_have_errors(&phases) {
        append_skipped(
            &mut phases,
            &[
                "remote-docker-network",
                "remote-compose-pull",
                "remote-compose-up",
                "remote-health",
            ],
            "skipped because remote asset setup failed",
        );
        return Ok(report(
            host,
            dry_run,
            &identity_values.home,
            &mcp_port,
            phases,
            started.elapsed().as_millis(),
        ));
    }

    phases.push(remote_phase(
        runner,
        host,
        "remote-docker-network",
        &format!(
            "docker network inspect {network} >/dev/null 2>&1 || docker network create {network}",
            network = shell_quote(&docker_network)
        ),
        None,
    )?);
    if phases_have_errors(&phases) {
        append_skipped(
            &mut phases,
            &["remote-compose-pull", "remote-compose-up", "remote-health"],
            "skipped because Docker network setup failed",
        );
        return Ok(report(
            host,
            dry_run,
            &identity_values.home,
            &mcp_port,
            phases,
            started.elapsed().as_millis(),
        ));
    }

    phases.push(remote_phase(
            runner,
            host,
            "remote-compose-pull",
            "docker compose --env-file ~/.syslog-mcp/.env -f ~/.syslog-mcp/compose/docker-compose.yml pull --ignore-buildable",
            None,
    )?);
    if phases_have_errors(&phases) {
        append_skipped(
            &mut phases,
            &["remote-compose-up", "remote-health"],
            "skipped because Compose pull failed",
        );
        return Ok(report(
            host,
            dry_run,
            &identity_values.home,
            &mcp_port,
            phases,
            started.elapsed().as_millis(),
        ));
    }

    phases.push(remote_phase(
            runner,
            host,
            "remote-compose-up",
            "docker compose --env-file ~/.syslog-mcp/.env -f ~/.syslog-mcp/compose/docker-compose.yml up -d",
            None,
    )?);
    if phases_have_errors(&phases) {
        append_skipped(
            &mut phases,
            &["remote-health"],
            "skipped because Compose up failed",
        );
        return Ok(report(
            host,
            dry_run,
            &identity_values.home,
            &mcp_port,
            phases,
            started.elapsed().as_millis(),
        ));
    }

    phases.push(remote_phase(
        runner,
        host,
        "remote-health",
        &format!(
            "curl -fsS {}",
            shell_quote(&format!("http://127.0.0.1:{mcp_port}/health"))
        ),
        None,
    )?);
    Ok(report(
        host,
        dry_run,
        &identity_values.home,
        &mcp_port,
        phases,
        started.elapsed().as_millis(),
    ))
}

struct RemoteIdentityPhase {
    phase: SetupPhase,
    values: Option<RemoteIdentity>,
}

struct RemoteIdentity {
    home: String,
    uid: String,
    gid: String,
}

fn remote_identity_phase(
    runner: &mut dyn RemoteRunner,
    host: &str,
) -> io::Result<RemoteIdentityPhase> {
    let timer = PhaseTimer::start("remote-identity");
    let output = match runner.run(host, "printf '%s\\n' \"$HOME\" && id -u && id -g", None) {
        Ok(output) => output,
        Err(err) => {
            return Ok(RemoteIdentityPhase {
                phase: timer.finish(SetupStatus::Error, format!("ssh failed: {err}")),
                values: None,
            });
        }
    };
    if output.status_success {
        let mut lines = output.stdout.lines();
        let home = lines.next().unwrap_or("$HOME").trim().to_string();
        let uid = lines.next().unwrap_or("1000").trim().to_string();
        let gid = lines.next().unwrap_or("1000").trim().to_string();
        return Ok(RemoteIdentityPhase {
            phase: timer.finish(SetupStatus::Ok, format!("home={home} uid={uid} gid={gid}")),
            values: Some(RemoteIdentity { home, uid, gid }),
        });
    }
    Ok(RemoteIdentityPhase {
        phase: timer.finish(SetupStatus::Error, output_detail(&output)),
        values: None,
    })
}

fn write_remote_env_phase(
    runner: &mut dyn RemoteRunner,
    host: &str,
    env: &std::collections::BTreeMap<String, String>,
) -> io::Result<SetupPhase> {
    let rendered = render_env(env);
    let script = format!(
        "umask 077\ncat > ~/.syslog-mcp/.env.tmp <<'__SYSLOG_MCP_ENV__'\n{rendered}__SYSLOG_MCP_ENV__\nchmod 600 ~/.syslog-mcp/.env.tmp\nmv ~/.syslog-mcp/.env.tmp ~/.syslog-mcp/.env"
    );
    remote_phase(runner, host, "remote-env", &script, None)
}

fn report(
    host: &str,
    dry_run: bool,
    remote_user_home: &str,
    mcp_port: &str,
    phases: Vec<SetupPhase>,
    elapsed_ms: u128,
) -> RemoteDeployReport {
    let remote_home = format!("{remote_user_home}/{REMOTE_HOME_SUFFIX}");
    let has_errors = phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error));
    RemoteDeployReport {
        mode: if dry_run { "remote dry-run" } else { "remote" },
        host: host.to_string(),
        home: remote_home.clone(),
        env_path: format!("{remote_home}/.env"),
        compose_dir: format!("{remote_home}/compose"),
        data_dir: format!("{remote_home}/data"),
        health_url: format!("http://127.0.0.1:{mcp_port}/health"),
        mcp_url: format!("http://127.0.0.1:{mcp_port}/mcp"),
        phases,
        has_errors,
        elapsed_ms,
    }
}

fn write_remote_assets_phase(runner: &mut dyn RemoteRunner, host: &str) -> io::Result<SetupPhase> {
    let script = format!(
        "cat > ~/.syslog-mcp/compose/docker-compose.yml.tmp <<'__SYSLOG_MCP_COMPOSE__'\n{}__SYSLOG_MCP_COMPOSE__\ncat > ~/.syslog-mcp/compose/config/Dockerfile.tmp <<'__SYSLOG_MCP_DOCKERFILE__'\n{}__SYSLOG_MCP_DOCKERFILE__\nmv ~/.syslog-mcp/compose/docker-compose.yml.tmp ~/.syslog-mcp/compose/docker-compose.yml\nmv ~/.syslog-mcp/compose/config/Dockerfile.tmp ~/.syslog-mcp/compose/config/Dockerfile",
        installed_compose_asset(),
        dockerfile_asset()
    );
    remote_phase(runner, host, "remote-compose-assets", &script, None)
}

fn remote_phase(
    runner: &mut dyn RemoteRunner,
    host: &str,
    name: &'static str,
    script: &str,
    stdin: Option<&str>,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start(name);
    match runner.run(host, script, stdin) {
        Ok(output) if output.status_success => {
            Ok(timer.finish(SetupStatus::Ok, output_detail(&output)))
        }
        Ok(output) => Ok(timer.finish(SetupStatus::Error, output_detail(&output))),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            Ok(timer.finish(SetupStatus::Error, "ssh not found on PATH"))
        }
        Err(err) => Ok(timer.finish(SetupStatus::Error, err.to_string())),
    }
}

fn skip_phase(name: &'static str, detail: &'static str) -> SetupPhase {
    PhaseTimer::start(name).finish(SetupStatus::Skipped, detail)
}

fn append_skipped(phases: &mut Vec<SetupPhase>, names: &[&'static str], detail: &'static str) {
    phases.extend(names.iter().map(|name| skip_phase(name, detail)));
}

fn phases_have_errors(phases: &[SetupPhase]) -> bool {
    phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn output_detail(output: &RemoteOutput) -> String {
    let text = if output.status_success {
        output.stdout.trim()
    } else if !output.stderr.trim().is_empty() {
        output.stderr.trim()
    } else {
        output.stdout.trim()
    };
    text.lines().last().unwrap_or("ok").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeRemoteRunner {
        commands: Vec<String>,
        fail_contains: Option<&'static str>,
        error_contains: Option<&'static str>,
    }

    impl FakeRemoteRunner {
        fn ok() -> Self {
            Self::default()
        }

        fn fail_on(needle: &'static str) -> Self {
            Self {
                commands: Vec::new(),
                fail_contains: Some(needle),
                error_contains: None,
            }
        }

        fn error_on(needle: &'static str) -> Self {
            Self {
                commands: Vec::new(),
                fail_contains: None,
                error_contains: Some(needle),
            }
        }
    }

    impl RemoteRunner for FakeRemoteRunner {
        fn run(
            &mut self,
            host: &str,
            script: &str,
            _stdin: Option<&str>,
        ) -> io::Result<RemoteOutput> {
            self.commands.push(format!("{host}: {script}"));
            if self
                .error_contains
                .is_some_and(|needle| script.contains(needle))
            {
                return Err(io::Error::new(io::ErrorKind::NotFound, "ssh not found"));
            }
            if self
                .fail_contains
                .is_some_and(|needle| script.contains(needle))
            {
                return Ok(RemoteOutput {
                    status_success: false,
                    stdout: String::new(),
                    stderr: "forced failure\n".to_string(),
                });
            }
            let stdout = if script.contains("id -u") {
                "/home/syslog\n1001\n1002\n".to_string()
            } else {
                "ok\n".to_string()
            };
            Ok(RemoteOutput {
                status_success: true,
                stdout,
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn remote_dry_run_only_checks_ssh_and_docker() {
        let mut runner = FakeRemoteRunner::ok();
        let report = run_remote_deploy_with_runner("host-a", true, &mut runner).unwrap();

        assert_eq!(report.host, "host-a");
        assert!(runner
            .commands
            .iter()
            .any(|cmd| cmd.contains("docker --version")));
        assert!(!runner
            .commands
            .iter()
            .any(|cmd| cmd.contains("docker compose") && cmd.contains("up -d")));
        assert!(!runner
            .commands
            .iter()
            .any(|cmd| cmd.contains("cat > ~/.syslog-mcp/.env")));
    }

    #[test]
    fn remote_repair_writes_assets_before_compose_up() {
        let mut runner = FakeRemoteRunner::ok();
        let report = run_remote_deploy_with_runner("host-a", false, &mut runner).unwrap();

        assert!(!report.has_errors);
        let index = |needle: &str| {
            runner
                .commands
                .iter()
                .position(|cmd| cmd.contains(needle))
                .unwrap_or_else(|| panic!("missing command containing {needle}"))
        };
        let mkdir = index("mkdir -p ~/.syslog-mcp/compose/config ~/.syslog-mcp/data");
        let docker_check = index("docker --version && docker compose version");
        let env_write = index("cat > ~/.syslog-mcp/.env.tmp");
        let assets_write = index("docker-compose.yml.tmp");
        let compose_pull = index("pull --ignore-buildable");
        let compose_up = index("up -d");
        assert!(mkdir < docker_check);
        assert!(docker_check < env_write);
        assert!(env_write < assets_write);
        assert!(assets_write < compose_pull);
        assert!(compose_pull < compose_up);
    }

    #[test]
    fn remote_env_uses_remote_uid_and_gid() {
        let mut runner = FakeRemoteRunner::ok();
        run_remote_deploy_with_runner("host-a", false, &mut runner).unwrap();

        let env_write = runner
            .commands
            .iter()
            .find(|cmd| cmd.contains("cat > ~/.syslog-mcp/.env.tmp"))
            .expect("env write command should run");
        assert!(env_write.contains("SYSLOG_UID=1001"));
        assert!(env_write.contains("SYSLOG_GID=1002"));
    }

    #[test]
    fn remote_deploy_skips_mutations_after_identity_failure() {
        let mut runner = FakeRemoteRunner::fail_on("id -u");
        let report = run_remote_deploy_with_runner("host-a", false, &mut runner).unwrap();

        assert!(report.has_errors);
        assert!(!runner.commands.iter().any(|cmd| cmd.contains("up -d")));
        assert!(report
            .phases
            .iter()
            .any(|phase| phase.name == "remote-compose-up"
                && matches!(phase.status, SetupStatus::Skipped)));
    }

    #[test]
    fn remote_deploy_reports_identity_spawn_error_as_phase_failure() {
        let mut runner = FakeRemoteRunner::error_on("id -u");
        let report = run_remote_deploy_with_runner("host-a", false, &mut runner).unwrap();

        assert!(report.has_errors);
        let identity = report
            .phases
            .iter()
            .find(|phase| phase.name == "remote-identity")
            .expect("identity phase should be present");
        assert!(matches!(identity.status, SetupStatus::Error));
        assert!(identity.detail.contains("ssh failed"));
        assert!(!runner.commands.iter().any(|cmd| cmd.contains("up -d")));
    }

    #[test]
    fn remote_deploy_does_not_source_env_as_shell() {
        let mut runner = FakeRemoteRunner::ok();
        run_remote_deploy_with_runner("host-a", false, &mut runner).unwrap();

        assert!(!runner
            .commands
            .iter()
            .any(|cmd| cmd.contains(". ~/.syslog-mcp/.env")));
    }
}
