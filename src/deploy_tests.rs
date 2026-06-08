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
    fn run(&mut self, host: &str, script: &str, _stdin: Option<&str>) -> io::Result<RemoteOutput> {
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
        .any(|cmd| cmd.contains("cat > ~/.cortex/.env")));
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
    let mkdir = index("mkdir -p ~/.cortex/compose/config ~/.cortex/data");
    let docker_check = index("docker --version && docker compose version");
    let env_write = index("cat > ~/.cortex/.env.tmp");
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
        .find(|cmd| cmd.contains("cat > ~/.cortex/.env.tmp"))
        .expect("env write command should run");
    assert!(env_write.contains("CORTEX_UID=1001"));
    assert!(env_write.contains("CORTEX_GID=1002"));
}

#[test]
fn remote_deploy_skips_mutations_after_identity_failure() {
    let mut runner = FakeRemoteRunner::fail_on("id -u");
    let report = run_remote_deploy_with_runner("host-a", false, &mut runner).unwrap();

    assert!(report.has_errors);
    assert!(!runner.commands.iter().any(|cmd| cmd.contains("up -d")));
    assert!(report.phases.iter().any(|phase| {
        phase.name == "remote-compose-up" && matches!(phase.status, SetupStatus::Skipped)
    }));
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
        .any(|cmd| cmd.contains(". ~/.cortex/.env")));
}

#[test]
fn remote_deploy_rejects_option_like_hosts_before_running_ssh() {
    let mut runner = FakeRemoteRunner::ok();

    let error = run_remote_deploy_with_runner("-oProxyCommand=touch /tmp/pwned", true, &mut runner)
        .unwrap_err();

    assert!(error.to_string().contains("unsafe ssh host"));
    assert!(runner.commands.is_empty());
}

#[test]
fn remote_deploy_accepts_safe_hosts() {
    let mut runner = FakeRemoteRunner::ok();

    let report = run_remote_deploy_with_runner("tootie", true, &mut runner).unwrap();

    assert_eq!(report.host, "tootie");
    assert!(runner
        .commands
        .iter()
        .all(|command| command.starts_with("tootie:")));
}
