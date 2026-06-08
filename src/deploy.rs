use std::io::{self, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::Serialize;

use crate::setup::{
    default_env_for_data_dir, dockerfile_asset, installed_compose_asset, render_env, PhaseTimer,
    SetupPhase, SetupStatus,
};

const REMOTE_HOME_SUFFIX: &str = ".cortex";

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
        let args = crate::inventory::ssh::SshContext::new(
            crate::inventory::ssh::SshOptions::from_env(None),
        )
        .ssh_args(host, "sh -s")
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        let mut child = Command::new("ssh")
            .args(args)
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
    if !crate::inventory::ssh::is_safe_ssh_host(host) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsafe ssh host: {host}"),
        ));
    }
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
    env.insert("CORTEX_DATA_VOLUME".to_string(), remote_data_dir.clone());
    env.insert("CORTEX_UID".to_string(), identity_values.uid.clone());
    env.insert("CORTEX_GID".to_string(), identity_values.gid.clone());
    let docker_network = env
        .get("DOCKER_NETWORK")
        .cloned()
        .unwrap_or_else(|| "cortex".to_string());
    let mcp_port = env
        .get("CORTEX_PORT")
        .cloned()
        .unwrap_or_else(|| "3100".to_string());

    if dry_run {
        phases.push(remote_phase(
            runner,
            host,
            "remote-filesystem",
            "test -d ~/.cortex || test -w \"$HOME\"",
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
        "mkdir -p ~/.cortex/compose/config ~/.cortex/data && chmod 700 ~/.cortex ~/.cortex/data",
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
            "docker compose --env-file ~/.cortex/.env -f ~/.cortex/compose/docker-compose.yml pull --ignore-buildable",
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
        "docker compose --env-file ~/.cortex/.env -f ~/.cortex/compose/docker-compose.yml up -d",
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
        "umask 077\ncat > ~/.cortex/.env.tmp <<'__CORTEX_ENV__'\n{rendered}__CORTEX_ENV__\nchmod 600 ~/.cortex/.env.tmp\nmv ~/.cortex/.env.tmp ~/.cortex/.env"
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
        "cat > ~/.cortex/compose/docker-compose.yml.tmp <<'__CORTEX_COMPOSE__'\n{}__CORTEX_COMPOSE__\ncat > ~/.cortex/compose/config/Dockerfile.tmp <<'__CORTEX_DOCKERFILE__'\n{}__CORTEX_DOCKERFILE__\nmv ~/.cortex/compose/docker-compose.yml.tmp ~/.cortex/compose/docker-compose.yml\nmv ~/.cortex/compose/config/Dockerfile.tmp ~/.cortex/compose/config/Dockerfile",
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
#[path = "deploy_tests.rs"]
mod tests;
