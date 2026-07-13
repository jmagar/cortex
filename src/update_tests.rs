use super::*;

#[test]
fn update_profile_round_trips_server_and_clients() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployments.toml");
    let profile = UpdateProfile {
        server: Some(ServerUpdateProfile {
            host: "tootie".to_string(),
            home: "/mnt/cache/appdata/cortex".to_string(),
        }),
        clients: ClientsUpdateProfile {
            hosts: vec!["dookie".to_string(), "shart".to_string()],
            target: Some("https://cortex.tootie.tv".to_string()),
            docker: Some(true),
            journald: None,
        },
    };

    write_profile(&path, &profile).unwrap();
    let loaded = load_profile(&path).unwrap();

    assert_eq!(loaded, profile);
}

#[test]
fn configure_server_profile_validates_and_preserves_clients() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployments.toml");
    write_profile(
        &path,
        &UpdateProfile {
            server: None,
            clients: ClientsUpdateProfile {
                hosts: vec!["dookie".to_string()],
                target: Some("https://cortex.tootie.tv".to_string()),
                docker: Some(true),
                journald: Some(false),
            },
        },
    )
    .unwrap();

    let updated =
        configure_server_profile(Some(&path), "tootie", "/mnt/cache/appdata/cortex").unwrap();

    assert_eq!(updated.server.as_ref().unwrap().host, "tootie");
    assert_eq!(
        updated.server.as_ref().unwrap().home,
        "/mnt/cache/appdata/cortex"
    );
    assert_eq!(updated.clients.hosts, vec!["dookie"]);
}

#[test]
fn configure_server_profile_rejects_unsafe_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployments.toml");

    let bad_host = configure_server_profile(
        Some(&path),
        "-oProxyCommand=touch /tmp/pwned",
        "/mnt/cache/appdata/cortex",
    )
    .unwrap_err();
    assert!(bad_host.to_string().contains("unsafe ssh host"));

    let bad_home = configure_server_profile(Some(&path), "tootie", "relative/path").unwrap_err();
    assert!(bad_home.to_string().contains("absolute path"));

    let parent_home =
        configure_server_profile(Some(&path), "tootie", "/mnt/cache/../cortex").unwrap_err();
    assert!(parent_home.to_string().contains("must not contain '..'"));
}

#[derive(Default)]
struct FakeUpdateRunner {
    server_calls: Vec<(String, RemoteDeployOptions)>,
    client_calls: Vec<String>,
    fail_server: bool,
    fail_client: Option<String>,
}

impl UpdateRunner for FakeUpdateRunner {
    fn run_server(
        &mut self,
        host: &str,
        options: RemoteDeployOptions,
    ) -> io::Result<RemoteDeployReport> {
        self.server_calls.push((host.to_string(), options.clone()));
        Ok(RemoteDeployReport {
            mode: if options.dry_run {
                "remote dry-run"
            } else {
                "remote"
            },
            host: host.to_string(),
            home: options.home.clone().unwrap(),
            env_path: format!("{}/.env", options.home.clone().unwrap()),
            compose_dir: format!("{}/compose", options.home.clone().unwrap()),
            data_dir: format!("{}/data", options.home.clone().unwrap()),
            health_url: "http://127.0.0.1:3100/health".to_string(),
            mcp_url: "http://127.0.0.1:3100/mcp".to_string(),
            phases: Vec::new(),
            has_errors: self.fail_server,
            elapsed_ms: 1,
        })
    }

    fn deploy_client(
        &mut self,
        host: &str,
        _binary: &Path,
        _config: &AgentDeployConfig,
    ) -> DeployResult {
        self.client_calls.push(host.to_string());
        let ok = self.fail_client.as_deref() != Some(host);
        DeployResult {
            host: host.to_string(),
            ok,
            detail: if ok {
                "ok".to_string()
            } else {
                "forced failure".to_string()
            },
            elapsed_ms: 1,
        }
    }

    fn find_binary(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/tmp/cortex"))
    }
}

#[test]
fn update_server_uses_saved_profile_without_repeating_home_arg() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployments.toml");
    configure_server_profile(Some(&path), "tootie", "/mnt/cache/appdata/cortex").unwrap();
    let mut runner = FakeUpdateRunner::default();

    let report = run_update_with_runner(
        UpdateScope::Server,
        UpdateOptions {
            dry_run: true,
            profile_path: Some(path.clone()),
            binary: None,
        },
        &mut runner,
    )
    .unwrap();

    assert!(!report.has_errors);
    assert_eq!(runner.server_calls.len(), 1);
    assert_eq!(runner.server_calls[0].0, "tootie");
    assert_eq!(
        runner.server_calls[0].1.home.as_deref(),
        Some("/mnt/cache/appdata/cortex")
    );
    assert!(runner.server_calls[0].1.dry_run);
    assert_eq!(report.profile_path, path);
}

#[test]
fn update_clients_deploys_every_configured_client() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployments.toml");
    configure_clients_profile(
        Some(&path),
        vec!["dookie".to_string(), "shart".to_string()],
        Some("https://cortex.tootie.tv".to_string()),
        Some(true),
        None,
    )
    .unwrap();
    let mut runner = FakeUpdateRunner::default();

    let report = run_update_with_runner(
        UpdateScope::Clients,
        UpdateOptions {
            dry_run: false,
            profile_path: Some(path),
            binary: Some(PathBuf::from("/tmp/cortex")),
        },
        &mut runner,
    )
    .unwrap();

    assert!(!report.has_errors);
    assert_eq!(runner.client_calls, vec!["dookie", "shart"]);
    assert_eq!(report.clients.len(), 2);
}

#[test]
fn update_all_stops_before_clients_when_server_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployments.toml");
    configure_server_profile(Some(&path), "tootie", "/mnt/cache/appdata/cortex").unwrap();
    configure_clients_profile(
        Some(&path),
        vec!["dookie".to_string()],
        Some("https://cortex.tootie.tv".to_string()),
        None,
        None,
    )
    .unwrap();
    let mut runner = FakeUpdateRunner {
        fail_server: true,
        ..FakeUpdateRunner::default()
    };

    let report = run_update_with_runner(
        UpdateScope::All,
        UpdateOptions {
            dry_run: false,
            profile_path: Some(path),
            binary: Some(PathBuf::from("/tmp/cortex")),
        },
        &mut runner,
    )
    .unwrap();

    assert!(report.has_errors);
    assert!(runner.client_calls.is_empty());
    assert!(report.skipped.iter().any(|phase| phase.name == "clients"));
}
