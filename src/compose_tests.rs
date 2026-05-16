use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Output;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use super::*;

fn cwd_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[derive(Default)]
struct FakeInspector {
    container: Option<ContainerInfo>,
    candidates: Vec<ContainerInfo>,
    systemd: Option<SystemdStatus>,
    listeners: Vec<ListenerInfo>,
    systemd_error: Option<String>,
    listeners_error: Option<String>,
    published_port_owners: BTreeMap<u16, String>,
}

impl DockerInspect for FakeInspector {
    fn inspect_container(&self, _name: &str) -> Result<Option<ContainerInfo>> {
        Ok(self.container.clone())
    }

    fn find_candidates(&self, _service: &str, _container_name: &str) -> Result<Vec<ContainerInfo>> {
        Ok(self.candidates.clone())
    }

    fn systemd_status(&self, _unit: &str) -> Result<Option<SystemdStatus>> {
        if let Some(error) = &self.systemd_error {
            anyhow::bail!("{error}");
        }
        Ok(self.systemd.clone())
    }

    fn listeners(&self, _ports: &[u16]) -> Result<Vec<ListenerInfo>> {
        if let Some(error) = &self.listeners_error {
            anyhow::bail!("{error}");
        }
        Ok(self.listeners.clone())
    }

    fn published_port_owner(&self, port: u16) -> Result<Option<String>> {
        Ok(self.published_port_owners.get(&port).cloned())
    }
}

#[derive(Default)]
struct FakeRunner;

impl CommandRunner for FakeRunner {
    fn run(&self, _invocation: &ComposeInvocation) -> Result<CommandOutput> {
        Ok(CommandOutput {
            exit_status: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            timed_out: false,
            timeout_cleanup: None,
        })
    }
}

fn labelled_container() -> ContainerInfo {
    let compose_file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docker-compose.yml");
    let project_dir = compose_file.parent().unwrap().to_path_buf();
    let mut labels = BTreeMap::new();
    labels.insert(
        "com.docker.compose.project".into(),
        "syslog-jmagar-lab".into(),
    );
    labels.insert(
        "com.docker.compose.project.working_dir".into(),
        project_dir.display().to_string(),
    );
    labels.insert(
        "com.docker.compose.project.config_files".into(),
        compose_file.display().to_string(),
    );
    labels.insert("com.docker.compose.service".into(), "syslog-mcp".into());
    ContainerInfo {
        id: "abc".into(),
        name: "syslog-mcp".into(),
        status: Some("Up".into()),
        health: Some("healthy".into()),
        image: Some("ghcr.io/jmagar/syslog-mcp:latest".into()),
        image_id: Some("sha256:abc".into()),
        labels,
        mounts: vec![MountInfo {
            source: Some(PathBuf::from("/home/jmagar/.claude/plugins/data/syslog-jmagar-lab")),
            target: "/data".into(),
            kind: "bind".into(),
        }],
        ports: vec![PortInfo {
            private_port: 3100,
            public_port: Some(3100),
            protocol: "tcp".into(),
            host_ip: Some("0.0.0.0".into()),
        }],
    }
}

fn unlabelled_container() -> ContainerInfo {
    ContainerInfo {
        labels: BTreeMap::new(),
        ..labelled_container()
    }
}

#[test]
fn redacts_sensitive_lines() {
    let input = "ok=true\nSYSLOG_MCP_TOKEN=abc\nclient_secret = \"secret\"\nport=3100";
    let redacted = redact_sensitive(input);
    assert!(redacted.contains("ok=true"));
    assert!(redacted.contains("port=3100"));
    assert!(!redacted.contains("abc"));
    assert!(!redacted.contains("client_secret"));
    assert_eq!(redacted.matches("[REDACTED]").count(), 2);
}

#[test]
fn mcp_projection_omits_host_paths_and_image_ids() {
    let status = ComposeStatus {
        container_name: "syslog-mcp".into(),
        container_id: Some("container-id".into()),
        status: Some("Up 1 minute".into()),
        health: Some("healthy".into()),
        image: Some("ghcr.io/jmagar/syslog-mcp:latest".into()),
        image_id: Some("sha256:secret-image-id".into()),
        compose_project: Some("syslog-jmagar-lab".into()),
        compose_working_dir: Some(PathBuf::from("/home/jmagar/private")),
        compose_files: vec![PathBuf::from("/home/jmagar/private/docker-compose.yml")],
        service: Some("syslog-mcp".into()),
        data_mounts: vec![MountInfo {
            source: Some(PathBuf::from("/home/jmagar/private/data")),
            target: "/data".into(),
            kind: "bind".into(),
        }],
        ports: vec![PortInfo {
            private_port: 3100,
            public_port: Some(3100),
            protocol: "tcp".into(),
            host_ip: Some("0.0.0.0".into()),
        }],
        systemd: None,
        diagnostics: vec![],
    };

    let projected = mcp_projection(&status);
    let json = serde_json::to_string(&projected).unwrap();
    assert_eq!(projected.ownership, ComposeOwnershipState::ComposeOwned);
    assert_eq!(projected.runtime_state, ComposeRuntimeState::Healthy);
    assert!(json.contains("3100"));
    assert!(!json.contains("/home/jmagar"));
    assert!(!json.contains("secret-image-id"));
    assert!(!json.contains("/data"));
}

#[test]
fn mcp_projection_treats_lowercase_docker_exited_as_stopped() {
    let status = ComposeStatus {
        container_name: "syslog-mcp".into(),
        container_id: Some("container-id".into()),
        status: Some("exited".into()),
        health: None,
        image: Some("ghcr.io/jmagar/syslog-mcp:latest".into()),
        image_id: Some("sha256:secret-image-id".into()),
        compose_project: Some("syslog-jmagar-lab".into()),
        compose_working_dir: None,
        compose_files: Vec::new(),
        service: Some("syslog-mcp".into()),
        data_mounts: Vec::new(),
        ports: Vec::new(),
        systemd: None,
        diagnostics: vec![],
    };

    let projected = mcp_projection(&status);

    assert_eq!(projected.runtime_state, ComposeRuntimeState::Stopped);
}

#[test]
fn mcp_projection_degrades_hard_diagnostics() {
    let mut status = ComposeStatus {
        container_name: "syslog-mcp".into(),
        container_id: Some("container-id".into()),
        status: Some("running".into()),
        health: Some("healthy".into()),
        image: Some("ghcr.io/jmagar/syslog-mcp:latest".into()),
        image_id: None,
        compose_project: Some("syslog-jmagar-lab".into()),
        compose_working_dir: None,
        compose_files: Vec::new(),
        service: Some("syslog-mcp".into()),
        data_mounts: Vec::new(),
        ports: Vec::new(),
        systemd: None,
        diagnostics: vec![ComposeDiagnostic {
            severity: DiagnosticSeverity::Unsafe,
            code: "incomplete_compose_labels".into(),
            message: "missing labels".into(),
        }],
    };

    let projected = mcp_projection(&status);
    assert_eq!(projected.ownership, ComposeOwnershipState::Unknown);
    assert_eq!(projected.runtime_state, ComposeRuntimeState::Degraded);
    assert!(ensure_doctor_ready(&status).is_err());

    status.diagnostics.clear();
    assert!(ensure_doctor_ready(&status).is_ok());
}

#[test]
fn ss_header_only_output_is_not_listener() {
    assert!(!ss_output_has_listener(
        b"Netid State Recv-Q Send-Q Local Address:Port Peer Address:Port Process\n"
    ));
    assert!(!ss_output_has_listener(b"\n"));
    assert!(ss_output_has_listener(
        b"tcp LISTEN 0 4096 0.0.0.0:3100 0.0.0.0:* users:((\"docker-proxy\",pid=123,fd=4))\n"
    ));
}

#[test]
fn resolves_live_container_labels() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = service.resolve_target(&ComposeTarget::default()).unwrap();
    assert_eq!(target.source, TargetSource::LiveContainerLabels);
    assert_eq!(target.confidence, TargetConfidence::Confirmed);
    assert_eq!(target.compose_project.as_deref(), Some("syslog-jmagar-lab"));
}

#[test]
fn inspect_json_extracts_compose_fields_ports_and_mounts() {
    let info = container_info_from_inspect(serde_json::json!({
        "Id": "abcdef123456",
        "Name": "/syslog-mcp",
        "Image": "sha256:image-id",
        "State": {
            "Status": "running",
            "Health": {"Status": "healthy"}
        },
        "Config": {
            "Image": "ghcr.io/jmagar/syslog-mcp:latest",
            "Labels": {
                "com.docker.compose.project": "syslog-jmagar-lab",
                "com.docker.compose.service": "syslog-mcp",
                "com.docker.compose.project.working_dir": "/srv/syslog",
                "com.docker.compose.project.config_files": "/srv/syslog/docker-compose.yml"
            }
        },
        "Mounts": [
            {"Type": "bind", "Source": "/srv/syslog/data", "Destination": "/data"}
        ],
        "NetworkSettings": {
            "Ports": {
                "3100/tcp": [{"HostIp": "0.0.0.0", "HostPort": "3100"}],
                "1514/udp": null
            }
        }
    }))
    .unwrap();

    assert_eq!(info.id, "abcdef123456");
    assert_eq!(info.name, "syslog-mcp");
    assert_eq!(info.status.as_deref(), Some("running"));
    assert_eq!(info.health.as_deref(), Some("healthy"));
    assert_eq!(
        info.image.as_deref(),
        Some("ghcr.io/jmagar/syslog-mcp:latest")
    );
    assert_eq!(info.image_id.as_deref(), Some("sha256:image-id"));
    assert_eq!(info.mounts[0].target, "/data");
    assert_eq!(info.ports.len(), 2);
    assert!(info
        .ports
        .iter()
        .any(|port| port.private_port == 3100 && port.public_port == Some(3100)));
    assert!(info
        .ports
        .iter()
        .any(|port| port.private_port == 1514 && port.public_port.is_none()));
}

#[test]
fn cwd_fallback_is_unsafe_and_refused_for_mutation() {
    let _guard = cwd_lock();
    let old_cwd = std::env::current_dir().unwrap();
    let tempdir = tempfile::tempdir().unwrap();
    std::fs::write(tempdir.path().join("docker-compose.yml"), "services: {}\n").unwrap();
    std::env::set_current_dir(tempdir.path()).unwrap();

    let service = ComposeService::new(
        FakeInspector::default(),
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = service.resolve_target(&ComposeTarget::default()).unwrap();
    assert_eq!(target.source, TargetSource::CurrentWorkingDirectory);
    assert_eq!(target.confidence, TargetConfidence::Unsafe);
    let err = service
        .preflight_mutation(ComposeMutation::Pull, &target, &MutationOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("cwd target"));

    std::env::set_current_dir(old_cwd).unwrap();
}

#[test]
fn requested_project_or_service_must_match_live_labels() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let err = service
        .resolve_target(&ComposeTarget {
            project_name: Some("staging".into()),
            ..Default::default()
        })
        .unwrap_err();
    assert!(err.to_string().contains("project_name"));

    let err = service
        .resolve_target(&ComposeTarget {
            service: Some("other".into()),
            ..Default::default()
        })
        .unwrap_err();
    assert!(err.to_string().contains("service"));
}

#[test]
fn matching_requested_selectors_are_accepted() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = service
        .resolve_target(&ComposeTarget {
            project_name: Some("syslog-jmagar-lab".into()),
            service: Some("syslog-mcp".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(target.confidence, TargetConfidence::Confirmed);
}

#[cfg(unix)]
fn output_with_status(code: i32, stdout: &str, stderr: &str) -> Output {
    Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[cfg(unix)]
#[test]
fn systemd_status_distinguishes_inactive_from_probe_failure() {
    let active = systemd_status_from_output("syslog-mcp.service", &output_with_status(0, "", ""))
        .unwrap()
        .unwrap();
    assert!(active.active);

    let inactive = systemd_status_from_output(
        "syslog-mcp.service",
        &output_with_status(3, "inactive\n", ""),
    )
    .unwrap()
    .unwrap();
    assert!(!inactive.active);

    let failed = systemd_status_from_output(
        "syslog-mcp.service",
        &output_with_status(1, "", "dbus unavailable"),
    )
    .unwrap_err();
    assert!(failed.to_string().contains("probe failure") || failed.to_string().contains("failed"));
}

#[test]
fn docker_unavailable_code_uses_typed_error() {
    let err: anyhow::Error = DockerUnavailableError("daemon is down".into()).into();
    assert_eq!(unresolved_code(&err), DIAG_DOCKER_UNAVAILABLE);

    let plain_err = anyhow::anyhow!("docker unavailable: text only");
    assert_eq!(unresolved_code(&plain_err), DIAG_TARGET_UNRESOLVED);
}

#[test]
fn status_reports_systemd_check_failures_as_diagnostics() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            systemd_error: Some("systemctl unavailable".into()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );

    let status = service.status(&ComposeTarget::default()).unwrap();

    assert_eq!(status.diagnostics[0].code, DIAG_SYSTEMD_CHECK_FAILED);
    assert_eq!(status.diagnostics[0].severity, DiagnosticSeverity::Error);
}

#[test]
fn status_errors_when_data_volume_is_named_not_bind() {
    // Regression guard: if SYSLOG_MCP_DATA_VOLUME is not substituted in the
    // compose invocation (missing --env-file), Docker creates a named volume
    // instead of a bind mount and the container writes to a separate database
    // from the CLI. The status check must detect and surface this as an Error.
    let mut container = labelled_container();
    container.mounts[0].kind = "volume".into(); // simulate named-volume drift
    let service = ComposeService::new(
        FakeInspector {
            container: Some(container),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );

    let status = service.status(&ComposeTarget::default()).unwrap();

    let drift = status
        .diagnostics
        .iter()
        .find(|d| d.code == "data_volume_not_bind")
        .expect("drift diagnostic must be present");
    assert_eq!(drift.severity, DiagnosticSeverity::Error);
}

#[test]
fn status_errors_when_data_volume_is_missing() {
    let mut container = labelled_container();
    container.mounts.clear(); // no /data mount at all
    let service = ComposeService::new(
        FakeInspector {
            container: Some(container),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );

    let status = service.status(&ComposeTarget::default()).unwrap();

    let missing = status
        .diagnostics
        .iter()
        .find(|d| d.code == "data_volume_missing")
        .expect("missing-volume diagnostic must be present");
    assert_eq!(missing.severity, DiagnosticSeverity::Error);
}

#[test]
fn containers_without_required_compose_labels_are_unsafe_for_mutation() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(unlabelled_container()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = service.resolve_target(&ComposeTarget::default()).unwrap();
    assert_eq!(target.source, TargetSource::LiveContainerLabels);
    assert_eq!(target.confidence, TargetConfidence::Unsafe);
    assert_eq!(target.diagnostics[0].code, "incomplete_compose_labels");
    let err = service
        .preflight_mutation(ComposeMutation::Down, &target, &MutationOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("required compose labels"));
}

#[test]
fn partial_compose_labels_are_unsafe_for_mutation() {
    let mut container = labelled_container();
    container
        .labels
        .remove("com.docker.compose.project.config_files");
    let service = ComposeService::new(
        FakeInspector {
            container: Some(container),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = service.resolve_target(&ComposeTarget::default()).unwrap();
    assert_eq!(target.confidence, TargetConfidence::Unsafe);
    let err = service
        .preflight_mutation(
            ComposeMutation::Restart,
            &target,
            &MutationOptions::default(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("required compose labels"));
}

#[test]
fn project_name_alone_is_rejected_for_mutation() {
    let service = ComposeService::new(
        FakeInspector::default(),
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = ResolvedComposeTarget {
        target: ComposeTargetSummary {
            project_dir: None,
            compose_file: None,
            project_name: Some("syslog".into()),
            service: "syslog-mcp".into(),
            container_name: "syslog-mcp".into(),
        },
        source: TargetSource::Explicit,
        confidence: TargetConfidence::Confirmed,
        diagnostics: Vec::new(),
        compose_files: Vec::new(),
        compose_working_dir: None,
        compose_project: Some("syslog".into()),
    };
    let err = service
        .preflight_mutation(ComposeMutation::Up, &target, &MutationOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("--project-name alone"));
}

#[test]
fn up_refuses_active_systemd_owner() {
    let service = ComposeService::new(
        FakeInspector {
            systemd: Some(SystemdStatus {
                unit: "syslog-mcp.service".into(),
                active: true,
            }),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    let err = service
        .preflight_mutation(ComposeMutation::Up, &target, &MutationOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("systemd"));
}

#[test]
fn mutation_refuses_unverified_systemd_or_listener_state() {
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    let systemd_service = ComposeService::new(
        FakeInspector {
            systemd_error: Some("systemctl unavailable".into()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let err = systemd_service
        .preflight_mutation(
            ComposeMutation::Restart,
            &target,
            &MutationOptions::default(),
        )
        .unwrap_err();
    assert!(err
        .to_string()
        .contains("could not verify systemd ownership"));

    let listener_service = ComposeService::new(
        FakeInspector {
            listeners_error: Some("ss unavailable".into()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let err = listener_service
        .preflight_mutation(
            ComposeMutation::Restart,
            &target,
            &MutationOptions::default(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("could not verify port listeners"));
}

#[test]
fn pull_does_not_require_systemd_or_listener_probes() {
    let service = ComposeService::new(
        FakeInspector {
            systemd_error: Some("systemctl unavailable".into()),
            listeners_error: Some("ss unavailable".into()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());

    service
        .preflight_mutation(ComposeMutation::Pull, &target, &MutationOptions::default())
        .unwrap();
}

#[test]
fn down_requires_yes_and_stops_only_target_service() {
    let service = ComposeService::new(
        FakeInspector::default(),
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    let err = service
        .preflight_mutation(
            ComposeMutation::Down,
            &target,
            &MutationOptions {
                non_interactive: true,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("--yes"));

    service
        .preflight_mutation(
            ComposeMutation::Down,
            &target,
            &MutationOptions {
                non_interactive: true,
                yes: true,
                ..Default::default()
            },
        )
        .unwrap();
    let invocation = service.compose_invocation(&target, ComposeMutation::Down);
    assert!(invocation
        .args
        .ends_with(&["stop".into(), "syslog-mcp".into()]));
    assert!(!invocation.args.iter().any(|arg| arg == "down"));
}

#[test]
fn up_refuses_non_target_listener() {
    let service = ComposeService::new(
        FakeInspector {
            listeners: vec![ListenerInfo {
                port: 3100,
                process: Some("users:((\"other\",pid=123,fd=7))".into()),
                belongs_to_target: false,
            }],
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let mut target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    target.source = TargetSource::Explicit;
    let err = service
        .preflight_mutation(ComposeMutation::Up, &target, &MutationOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("non-target listener"));
}

#[test]
fn live_target_allows_listener_on_published_target_port() {
    let mut owners = BTreeMap::new();
    owners.insert(3100, "abc".into());
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            listeners: vec![ListenerInfo {
                port: 3100,
                process: Some("users:((\"docker-proxy\",pid=123,fd=7))".into()),
                belongs_to_target: false,
            }],
            published_port_owners: owners,
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    service
        .preflight_mutation(ComposeMutation::Up, &target, &MutationOptions::default())
        .unwrap();
}

#[test]
fn live_target_refuses_foreign_docker_proxy_on_published_port() {
    let mut owners = BTreeMap::new();
    owners.insert(3100, "foreign-container".into());
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            listeners: vec![ListenerInfo {
                port: 3100,
                process: Some("users:((\"docker-proxy\",pid=123,fd=7))".into()),
                belongs_to_target: false,
            }],
            published_port_owners: owners,
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    let err = service
        .preflight_mutation(ComposeMutation::Up, &target, &MutationOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("non-target listener"));
}

#[test]
fn live_target_refuses_listener_on_unpublished_target_port() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            listeners: vec![ListenerInfo {
                port: 1514,
                process: Some("other".into()),
                belongs_to_target: false,
            }],
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    let err = service
        .preflight_mutation(ComposeMutation::Up, &target, &MutationOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("non-target listener"));
}

#[test]
fn live_target_refuses_foreign_listener_even_on_published_port() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            listeners: vec![ListenerInfo {
                port: 3100,
                process: Some("users:((\"other\",pid=123,fd=7))".into()),
                belongs_to_target: false,
            }],
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    let err = service
        .preflight_mutation(ComposeMutation::Up, &target, &MutationOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("non-target listener"));
}

#[test]
fn live_target_refuses_published_listener_with_unknown_owner() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            listeners: vec![ListenerInfo {
                port: 3100,
                process: Some("LISTEN 0 4096 0.0.0.0:3100 0.0.0.0:*".into()),
                belongs_to_target: false,
            }],
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    let err = service
        .preflight_mutation(ComposeMutation::Up, &target, &MutationOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("non-target listener"));
}

#[test]
fn live_target_allows_listener_without_process_info_when_docker_confirms_owner() {
    // Non-root scenario: ss cannot report process names so the listener has no
    // "users:" field, but docker ps confirms the target container owns the port.
    let mut owners = BTreeMap::new();
    owners.insert(3100, "abc".into());
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            listeners: vec![ListenerInfo {
                port: 3100,
                process: Some("LISTEN 0 4096 0.0.0.0:3100 0.0.0.0:*".into()),
                belongs_to_target: false,
            }],
            published_port_owners: owners,
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    service
        .preflight_mutation(ComposeMutation::Up, &target, &MutationOptions::default())
        .unwrap();
}

#[test]
fn up_invocation_is_detached_and_uses_project_directory_and_all_files() {
    let service = ComposeService::new(
        FakeInspector::default(),
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = ResolvedComposeTarget {
        target: ComposeTargetSummary {
            project_dir: Some(PathBuf::from("/tmp/project")),
            compose_file: Some(PathBuf::from("/tmp/project/base.yml")),
            project_name: Some("syslog-jmagar-lab".into()),
            service: "syslog-mcp".into(),
            container_name: "syslog-mcp".into(),
        },
        source: TargetSource::LiveContainerLabels,
        confidence: TargetConfidence::Confirmed,
        diagnostics: Vec::new(),
        compose_files: vec![
            PathBuf::from("/tmp/project/base.yml"),
            PathBuf::from("/tmp/project/override.yml"),
        ],
        compose_working_dir: Some(PathBuf::from("/tmp/project")),
        compose_project: Some("syslog-jmagar-lab".into()),
    };

    let invocation = service.compose_invocation(&target, ComposeMutation::Up);
    assert_eq!(invocation.program, "docker");
    assert_eq!(invocation.current_dir, Some(PathBuf::from("/tmp/project")));
    assert_eq!(
        invocation.args,
        vec![
            "compose",
            "--project-directory",
            "/tmp/project",
            "-f",
            "/tmp/project/base.yml",
            "-f",
            "/tmp/project/override.yml",
            "--project-name",
            "syslog-jmagar-lab",
            "up",
            "-d",
            "syslog-mcp",
        ]
    );
}

#[test]
fn mutation_invocations_scope_service_where_supported() {
    let service = ComposeService::new(
        FakeInspector::default(),
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    for (mutation, expected) in [
        (ComposeMutation::Pull, vec!["pull", "syslog-mcp"]),
        (ComposeMutation::Restart, vec!["restart", "syslog-mcp"]),
        (ComposeMutation::Down, vec!["stop", "syslog-mcp"]),
    ] {
        let invocation = service.compose_invocation(&target, mutation);
        assert!(
            invocation
                .args
                .ends_with(&expected.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
            "unexpected args for {mutation:?}: {:?}",
            invocation.args
        );
    }
}

#[test]
fn dry_run_does_not_invoke_runner() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let result = service
        .run_mutation(
            ComposeMutation::Pull,
            &ComposeTarget::default(),
            &MutationOptions {
                dry_run: true,
                ..Default::default()
            },
        )
        .unwrap();
    let ComposeCommandResult::DryRun(dry_run) = result else {
        panic!("expected dry-run result");
    };
    assert!(dry_run.dry_run);
    assert!(dry_run
        .command
        .ends_with(&["pull".into(), "syslog-mcp".into()]));
    assert_eq!(dry_run.preflight, "passed");
}

#[test]
fn logs_invocation_is_bounded_tail() {
    let service = ComposeService::new(
        FakeInspector::default(),
        FakeRunner,
        ComposeDefaults::default(),
    );
    let target = target_from_container(&labelled_container(), &ComposeDefaults::default());
    let invocation = service.logs_invocation(&target, 20);
    assert!(invocation.args.ends_with(&[
        "logs".into(),
        "--tail".into(),
        "20".into(),
        "syslog-mcp".into(),
    ]));
}

#[test]
fn logs_refuses_unsafe_target() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(unlabelled_container()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let err = service
        .logs(&ComposeTarget::default(), Some(20))
        .unwrap_err();
    assert!(err.to_string().contains("target is not confirmed"));
}

#[cfg(unix)]
#[test]
fn process_runner_truncates_and_redacts_output() {
    let runner = ProcessRunner;
    let invocation = ComposeInvocation {
        program: "sh".into(),
        args: vec![
            "-c".into(),
            "printf 'token=secret-value\\nvisible-line\\nmore-output\\n'".into(),
        ],
        current_dir: None,
        timeout: Duration::from_secs(5),
        output_limit_bytes: 32,
    };
    let output = runner.run(&invocation).unwrap();
    assert_eq!(output.exit_status, Some(0));
    assert!(output.stdout.contains("[REDACTED]"));
    assert!(!output.stdout.contains("secret-value"));
    assert!(output.stdout_truncated);
}

#[cfg(unix)]
#[test]
fn process_runner_times_out_and_reports_cleanup() {
    let runner = ProcessRunner;
    let invocation = ComposeInvocation {
        program: "sh".into(),
        args: vec!["-c".into(), "sleep 5".into()],
        current_dir: None,
        timeout: Duration::from_millis(100),
        output_limit_bytes: 1024,
    };
    let output = runner.run(&invocation).unwrap();
    assert!(output.timed_out);
    assert!(output.timeout_cleanup.as_ref().is_some_and(|c| c.reaped));
}

#[cfg(unix)]
#[test]
fn process_runner_kills_term_ignoring_process_group() {
    let runner = ProcessRunner;
    let invocation = ComposeInvocation {
        program: "sh".into(),
        args: vec![
            "-c".into(),
            "trap '' TERM; sh -c 'trap \"\" TERM; while true; do sleep 1; done' & wait".into(),
        ],
        current_dir: None,
        timeout: Duration::from_millis(100),
        output_limit_bytes: 1024,
    };
    let output = runner.run(&invocation).unwrap();
    assert!(output.timed_out);
    let cleanup = output.timeout_cleanup.unwrap();
    assert!(cleanup.terminate_sent);
    assert!(cleanup.kill_sent);
    assert!(cleanup.reaped);
}
