use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use super::*;

#[derive(Default)]
struct FakeInspector {
    container: Option<ContainerInfo>,
    candidates: Vec<ContainerInfo>,
    systemd: Option<SystemdStatus>,
    listeners: Vec<ListenerInfo>,
}

impl DockerInspect for FakeInspector {
    fn inspect_container(&self, _name: &str) -> Result<Option<ContainerInfo>> {
        Ok(self.container.clone())
    }

    fn find_candidates(&self, _service: &str, _container_name: &str) -> Result<Vec<ContainerInfo>> {
        Ok(self.candidates.clone())
    }

    fn systemd_status(&self, _unit: &str) -> Result<Option<SystemdStatus>> {
        Ok(self.systemd.clone())
    }

    fn listeners(&self, _ports: &[u16]) -> Result<Vec<ListenerInfo>> {
        Ok(self.listeners.clone())
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
    let compose_file = std::env::current_dir().unwrap().join("docker-compose.yml");
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
        mounts: Vec::new(),
        ports: vec![PortInfo {
            private_port: 3100,
            public_port: Some(3100),
            protocol: "tcp".into(),
            host_ip: Some("0.0.0.0".into()),
        }],
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
fn up_refuses_non_target_listener() {
    let service = ComposeService::new(
        FakeInspector {
            listeners: vec![ListenerInfo {
                port: 3100,
                process: Some("other".into()),
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
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            listeners: vec![ListenerInfo {
                port: 3100,
                process: Some("docker-proxy".into()),
                belongs_to_target: false,
            }],
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
fn dry_run_does_not_invoke_runner() {
    let service = ComposeService::new(
        FakeInspector {
            container: Some(labelled_container()),
            ..Default::default()
        },
        FakeRunner,
        ComposeDefaults::default(),
    );
    let output = service
        .run_mutation(
            ComposeMutation::Pull,
            &ComposeTarget::default(),
            &MutationOptions {
                dry_run: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(output.is_none());
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
