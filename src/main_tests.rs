use super::{Mode, cli};

#[test]
fn mode_parse_accepts_single_binary_transport_commands() {
    assert_eq!(Mode::parse(vec![]).unwrap(), Mode::ServeMcp);
    assert_eq!(
        Mode::parse(vec!["serve".into(), "mcp".into()]).unwrap(),
        Mode::ServeMcp
    );
    assert_eq!(Mode::parse(vec!["mcp".into()]).unwrap(), Mode::StdioMcp);
    assert_eq!(Mode::parse(vec!["--help".into()]).unwrap(), Mode::Help);
    assert_eq!(
        Mode::parse(vec!["--version".into()]).unwrap(),
        Mode::Version
    );
    assert!(matches!(
        Mode::parse(vec!["stats".into(), "--json".into()]).unwrap(),
        Mode::Cli(_)
    ));
}

#[test]
fn mode_parse_accepts_heartbeat_state_commands() {
    // Regression: host-state/fleet-state/correlate-state are routed in
    // parse.rs + run.rs, but were missing from Mode::parse's top-level command
    // gate, so they fell through to print_usage()+exit 1 (bd syslog-mcp-8fww).
    assert!(matches!(
        Mode::parse(vec![
            "host-state".into(),
            "--host".into(),
            "tootie".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Cli(_)
    ));
    assert!(matches!(
        Mode::parse(vec!["fleet-state".into(), "--json".into()]).unwrap(),
        Mode::Cli(_)
    ));
    assert!(matches!(
        Mode::parse(vec![
            "correlate-state".into(),
            "--reference-time".into(),
            "2026-01-01T00:00:00Z".into(),
            "--json".into(),
        ])
        .unwrap(),
        Mode::Cli(_)
    ));
}

#[test]
fn mode_parse_accepts_topic_correlate() {
    // Regression: topic-correlate is in TOP_LEVEL_COMMANDS + parse_command but was
    // missing from Mode::parse's top-level command gate, so `cortex topic-correlate
    // <topic>` parsed successfully and then hit the `unreachable!()` fallthrough →
    // panic. Both the single-term and the multi-term (screenshot) forms must route
    // cleanly to a CLI invocation.
    assert!(matches!(
        Mode::parse(vec!["topic-correlate".into(), "axon".into()]).unwrap(),
        Mode::Cli(_)
    ));
    assert!(matches!(
        Mode::parse(vec![
            "topic-correlate".into(),
            "squirts".into(),
            "dockersocket".into(),
        ])
        .unwrap(),
        Mode::Cli(_)
    ));
}

#[test]
fn mode_parse_rejects_unknown_commands() {
    let err = Mode::parse(vec!["serve".into(), "http".into()]).unwrap_err();
    assert!(err.to_string().contains("unknown CLI command"));

    let err = Mode::parse(vec!["serach".into()]).unwrap_err();
    assert!(err.to_string().contains("Did you mean `search`?"));
}

#[test]
fn mode_parse_keeps_runtime_status_mcp_only() {
    let err = Mode::parse(vec!["status".into()]).unwrap_err();
    assert!(err.to_string().contains("unknown CLI command"));
}

#[test]
fn mode_parse_accepts_ai_namespace() {
    assert!(matches!(
        Mode::parse(vec!["ai".into(), "tools".into(), "--json".into()]).unwrap(),
        Mode::Cli(_)
    ));
}

#[test]
fn mode_parse_accepts_graph_cli_commands() {
    assert!(matches!(
        Mode::parse(vec![
            "entity".into(),
            "host".into(),
            "tootie".into(),
            "--json".into(),
        ])
        .unwrap(),
        Mode::Cli(_)
    ));
    assert!(matches!(
        Mode::parse(vec![
            "graph".into(),
            "around".into(),
            "host:tootie".into(),
            "--limit".into(),
            "1".into(),
            "--json".into(),
        ])
        .unwrap(),
        Mode::Cli(_)
    ));
}

#[test]
fn mode_parse_accepts_compose_namespace() {
    assert!(matches!(
        Mode::parse(vec!["compose".into(), "status".into(), "--json".into()]).unwrap(),
        Mode::Cli(_)
    ));
}

#[test]
fn mode_parse_accepts_db_namespace() {
    assert!(matches!(
        Mode::parse(vec!["db".into(), "status".into(), "--json".into()]).unwrap(),
        Mode::Cli(_)
    ));
}

#[test]
fn mode_parse_accepts_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "plugin-hook".into(),
            "--no-repair".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Cli(_)
    ));
    assert!(matches!(
        Mode::parse(vec!["setup".into(), "check".into(), "--json".into()]).unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_sessions_index_timer_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "sessions-index-timer".into(),
            "install".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_sessions_watch_service_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "sessions-watch-service".into(),
            "install".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_rejects_old_ai_setup_namespaces() {
    for old_name in ["ai-index-timer", "ai-watch-service"] {
        let err = Mode::parse(vec!["setup".into(), old_name.into(), "check".into()]).unwrap_err();
        assert!(
            err.to_string().contains("unknown setup argument"),
            "expected {old_name} to be rejected, got: {err}"
        );
    }
}

#[test]
fn mode_parse_accepts_agent_command_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "agent-command".into(),
            "install".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_heartbeat_agent_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "heartbeat-agent".into(),
            "install".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_command_ingest_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "shell".into(),
            "index".into(),
            "--path".into(),
            "/tmp/history".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Cli(_)
    ));
    assert!(matches!(
        Mode::parse(vec![
            "heartbeat".into(),
            "agent".into(),
            "--server".into(),
            "http://127.0.0.1:3100".into(),
            "--token".into(),
            "secret".into(),
            "--emit".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Cli(_)
    ));
    assert!(matches!(
        Mode::parse(vec![
            "agent-command".into(),
            "ingest-spool".into(),
            "--path".into(),
            "/tmp/spool.jsonl".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Cli(_)
    ));
    assert!(matches!(
        Mode::parse(vec![
            "agent-command".into(),
            "wrap".into(),
            "--spool".into(),
            "/tmp/spool.jsonl".into(),
            "--".into(),
            "true".into()
        ])
        .unwrap(),
        Mode::Cli(_)
    ));
}

#[test]
fn mode_parse_preserves_wrapped_command_http_like_flags() {
    let mode = Mode::parse(vec![
        "agent-command".into(),
        "wrap".into(),
        "--spool".into(),
        "/tmp/spool.jsonl".into(),
        "--".into(),
        "curl".into(),
        "--http".into(),
        "--server".into(),
        "https://example.test".into(),
        "--token=literal".into(),
    ])
    .unwrap();

    let Mode::Cli(invocation) = mode else {
        panic!("expected CLI mode");
    };
    assert_eq!(invocation.flags, cli::GlobalFlags::default());
    let cli::CliCommand::AgentCommand(cli::AgentCommandCommand::Wrap(args)) = invocation.command
    else {
        panic!("expected agent-command wrap");
    };
    assert_eq!(
        args.command,
        vec![
            "curl".to_string(),
            "--http".to_string(),
            "--server".to_string(),
            "https://example.test".to_string(),
            "--token=literal".to_string(),
        ]
    );
}

#[test]
fn mode_parse_accepts_debug_wrapper_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "debug-wrapper".into(),
            "check".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_debug_compose_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "debug-compose".into(),
            "check".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_setup_doctor_namespace() {
    assert!(matches!(
        Mode::parse(vec!["setup".into(), "doctor".into(), "--json".into()]).unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_deploy_namespace() {
    assert!(matches!(
        Mode::parse(vec!["deploy".into(), "preflight".into(), "--json".into()]).unwrap(),
        Mode::Deploy(_)
    ));
    assert!(matches!(
        Mode::parse(vec![
            "deploy".into(),
            "local".into(),
            "--dry-run".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Deploy(_)
    ));
    assert!(matches!(
        Mode::parse(vec![
            "deploy".into(),
            "remote".into(),
            "tootie".into(),
            "--dry-run".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Deploy(_)
    ));
    assert!(matches!(
        Mode::parse(vec![
            "deploy".into(),
            "agent".into(),
            "--hosts".into(),
            "tootie,dookie".into(),
            "--target".into(),
            "https://cortex.example.test".into(),
            "--heartbeat-token".into(),
            "secret".into(),
            "--binary".into(),
            "/tmp/cortex".into(),
            "--docker".into(),
            "--journald".into(),
            "--json".into(),
        ])
        .unwrap(),
        Mode::Deploy(_)
    ));
}

#[test]
fn mode_parse_rejects_unknown_deploy_subcommand() {
    let err = Mode::parse(vec!["deploy".into(), "bogus".into()]).unwrap_err();
    assert!(err.to_string().contains("unknown deploy subcommand: bogus"));
}

#[test]
fn mode_parse_rejects_remote_deploy_without_host() {
    let err = Mode::parse(vec!["deploy".into(), "remote".into()]).unwrap_err();
    assert!(err.to_string().contains("deploy remote requires a host"));
}

#[test]
fn mode_parse_rejects_remote_deploy_with_multiple_hosts() {
    let err = Mode::parse(vec![
        "deploy".into(),
        "remote".into(),
        "host-a".into(),
        "host-b".into(),
    ])
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("deploy remote accepts exactly one host")
    );
}

#[test]
fn parse_deploy_agent_trims_and_drops_empty_hosts() {
    let command = super::parse_deploy_command(&[
        "agent".into(),
        "--hosts".into(),
        " tootie, ,dookie, ".into(),
    ])
    .unwrap();

    let super::DeployCommandKind::Agent { hosts, .. } = command.kind else {
        panic!("expected deploy agent");
    };
    assert_eq!(hosts, vec!["tootie".to_string(), "dookie".to_string()]);
}

#[test]
fn parse_deploy_agent_preserves_all_options() {
    let command = super::parse_deploy_command(&[
        "agent".into(),
        "--hosts".into(),
        "tootie,dookie".into(),
        "--target".into(),
        "https://cortex.example.test".into(),
        "--heartbeat-token".into(),
        "secret".into(),
        "--binary".into(),
        "/tmp/cortex".into(),
        "--docker".into(),
        "--journald".into(),
        "--json".into(),
    ])
    .unwrap();

    assert!(command.json);
    let super::DeployCommandKind::Agent {
        hosts,
        target,
        token,
        docker,
        journald,
        binary,
    } = command.kind
    else {
        panic!("expected deploy agent");
    };
    assert_eq!(hosts, vec!["tootie".to_string(), "dookie".to_string()]);
    assert_eq!(target.as_deref(), Some("https://cortex.example.test"));
    assert_eq!(token.as_deref(), Some("secret"));
    assert_eq!(docker, Some(true));
    assert_eq!(journald, Some(true));
    assert_eq!(binary.as_deref(), Some("/tmp/cortex"));
}

#[test]
fn parse_deploy_agent_reports_missing_option_values() {
    for flag in ["--hosts", "--target", "--heartbeat-token", "--binary"] {
        let err = super::parse_deploy_command(&["agent".into(), flag.into()]).unwrap_err();
        assert!(
            err.to_string().contains("requires a value"),
            "expected missing-value error for {flag}, got: {err}"
        );
    }
}

#[test]
fn mode_parse_setup_subcommands_default_to_check_and_parse_remove() {
    let cases = [
        (
            vec!["setup", "sessions-index-timer", "--json"],
            "sessions-index-timer check",
        ),
        (
            vec!["setup", "sessions-watch-service", "remove", "--json"],
            "sessions-watch-service remove",
        ),
        (
            vec!["setup", "agent-command", "remove", "--json"],
            "agent-command remove",
        ),
        (
            vec!["setup", "heartbeat-agent", "remove", "--json"],
            "heartbeat-agent remove",
        ),
        (
            vec!["setup", "debug-wrapper", "remove", "--json"],
            "debug-wrapper remove",
        ),
        (
            vec!["setup", "debug-compose", "remove", "--json"],
            "debug-compose remove",
        ),
    ];

    for (args, label) in cases {
        let mode = Mode::parse(args.into_iter().map(str::to_string).collect()).unwrap();
        let Mode::Setup(command) = mode else {
            panic!("{label}: expected setup mode");
        };
        assert!(command.json, "{label}: --json should be preserved");
    }
}

#[test]
fn mode_parse_rejects_duplicate_sessions_watch_service_actions() {
    let err = Mode::parse(vec![
        "setup".into(),
        "sessions-watch-service".into(),
        "install".into(),
        "remove".into(),
    ])
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("sessions-watch-service action specified more than once")
    );
}

#[test]
fn mode_parse_accepts_binary_doctor() {
    assert!(matches!(
        Mode::parse(vec!["doctor".into(), "binary".into(), "--json".into()]).unwrap(),
        Mode::DoctorBinary(_)
    ));
}

#[test]
fn mode_default_log_filter_matches_operational_noise_profile() {
    assert_eq!(Mode::ServeMcp.default_log_filter(), "info");
    assert_eq!(Mode::StdioMcp.default_log_filter(), "warn");
    assert_eq!(Mode::Help.default_log_filter(), "info");
    assert_eq!(Mode::Version.default_log_filter(), "info");
    assert_eq!(
        Mode::Setup(super::SetupCommand {
            kind: super::SetupCommandKind::Main(cortex::setup::SetupMode::Check),
            json: false,
        })
        .default_log_filter(),
        "warn"
    );
    assert_eq!(
        Mode::Deploy(super::DeployCommand {
            kind: super::DeployCommandKind::Preflight,
            json: false,
        })
        .default_log_filter(),
        "warn"
    );
    assert_eq!(
        Mode::DoctorBinary(super::DoctorBinaryCommand { json: false }).default_log_filter(),
        "warn"
    );
    assert_eq!(
        Mode::DoctorFull(super::DoctorFullCommand { json: false }).default_log_filter(),
        "warn"
    );
    assert_eq!(
        Mode::parse(vec!["search".into(), "foo".into()])
            .unwrap()
            .default_log_filter(),
        "error"
    );
}

#[test]
fn parse_doctor_commands_accept_json_and_reject_bad_shapes() {
    assert!(
        super::parse_doctor_full_command(&["--json".into()])
            .unwrap()
            .json
    );
    assert!(
        super::parse_doctor_command(&["binary".into(), "--json".into()])
            .unwrap()
            .json
    );

    let err = super::parse_doctor_command(&["--json".into()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("doctor requires `binary`"));

    let err = super::parse_doctor_full_command(&["bogus".into()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown doctor argument"));
}

#[test]
fn parse_setup_command_accepts_main_modes_and_rejects_bad_args() {
    let check = super::parse_setup_command(&["check".into(), "--json".into()]).unwrap();
    assert!(matches!(
        check.kind,
        super::SetupCommandKind::Main(cortex::setup::SetupMode::Check)
    ));
    assert!(check.json);

    let repair = super::parse_setup_command(&["repair".into()]).unwrap();
    assert!(matches!(
        repair.kind,
        super::SetupCommandKind::Main(cortex::setup::SetupMode::Repair)
    ));

    let default = super::parse_setup_command(&[]).unwrap();
    assert!(matches!(
        default.kind,
        super::SetupCommandKind::Main(cortex::setup::SetupMode::FirstRun)
    ));

    let err = super::parse_setup_command(&["bogus".into()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown setup argument"));

    let err =
        super::parse_setup_command(&["debug-wrapper".into(), "install".into(), "remove".into()])
            .unwrap_err()
            .to_string();
    assert!(err.contains("debug-wrapper action specified more than once"));

    let err = super::parse_setup_command(&["debug-compose".into(), "--bad".into()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown debug-compose argument"));
}

#[test]
fn parse_deploy_command_covers_modes_and_rejects_contextual_flags() {
    let preflight = super::parse_deploy_command(&["preflight".into(), "--json".into()]).unwrap();
    assert!(matches!(
        preflight.kind,
        super::DeployCommandKind::Preflight
    ));
    assert!(preflight.json);

    let local = super::parse_deploy_command(&["local".into()]).unwrap();
    assert!(matches!(
        local.kind,
        super::DeployCommandKind::Local { dry_run: false }
    ));

    let remote =
        super::parse_deploy_command(&["remote".into(), "tootie".into(), "--dry-run".into()])
            .unwrap();
    assert!(matches!(
        remote.kind,
        super::DeployCommandKind::Remote {
            ref host,
            dry_run: true
        } if host == "tootie"
    ));

    for (args, expected) in [
        (
            vec!["local", "--hosts", "tootie"],
            "unknown deploy local argument",
        ),
        (vec!["remote", "--docker"], "unknown deploy remote argument"),
        (vec!["agent", "--dry-run"], "unknown deploy agent argument"),
    ] {
        let err =
            super::parse_deploy_command(&args.into_iter().map(str::to_string).collect::<Vec<_>>())
                .unwrap_err()
                .to_string();
        assert!(err.contains(expected), "expected {expected:?}, got {err:?}");
    }
}

// ─── Bead 0p8r.6: global HTTP flag plumbing ─────────────────────────────────

use serial_test::serial;

/// Restores CORTEX_API_TOKEN / CORTEX_USE_HTTP on drop. Tests below use this
/// to assert `--help` / `--version` work without any token in the env.
struct EnvVarGuard {
    name: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn unset(name: &'static str) -> Self {
        let previous = std::env::var(name).ok();
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var(name) };
        Self { name, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            // TODO: Audit that the environment access only happens in single-threaded code.
            Some(v) => unsafe { std::env::set_var(self.name, v) },
            // TODO: Audit that the environment access only happens in single-threaded code.
            None => unsafe { std::env::remove_var(self.name) },
        }
    }
}

#[test]
#[serial]
fn help_and_version_bypass_mode_resolution_without_token() {
    // Bead .6 contract: --help and --version must NOT touch env / discovery /
    // services. Without CORTEX_API_TOKEN and CORTEX_USE_HTTP set, they should
    // still resolve to Help/Version without erroring.
    let _g1 = EnvVarGuard::unset("CORTEX_API_TOKEN");
    let _g2 = EnvVarGuard::unset("CORTEX_USE_HTTP");

    assert_eq!(Mode::parse(vec!["--help".into()]).unwrap(), Mode::Help);
    assert_eq!(Mode::parse(vec!["-h".into()]).unwrap(), Mode::Help);
    assert_eq!(Mode::parse(vec!["help".into()]).unwrap(), Mode::Help);
    assert_eq!(
        Mode::parse(vec!["--version".into()]).unwrap(),
        Mode::Version
    );
    assert_eq!(Mode::parse(vec!["-V".into()]).unwrap(), Mode::Version);
    assert_eq!(Mode::parse(vec!["version".into()]).unwrap(), Mode::Version);
}

#[test]
fn mode_parse_strips_http_flag_before_subcommand_dispatch() {
    // `--http` before the subcommand keyword must not break dispatch.
    let mode = Mode::parse(vec!["--http".into(), "search".into(), "foo".into()]).unwrap();
    let invocation = match mode {
        Mode::Cli(b) => *b,
        other => panic!("expected Cli, got {other:?}"),
    };
    assert!(invocation.flags.force_http);
}

#[test]
fn mode_parse_strips_http_flag_after_subcommand_dispatch() {
    // `--http` after the subcommand keyword also works.
    let mode = Mode::parse(vec!["search".into(), "--http".into(), "foo".into()]).unwrap();
    let invocation = match mode {
        Mode::Cli(b) => *b,
        other => panic!("expected Cli, got {other:?}"),
    };
    assert!(invocation.flags.force_http);
}

#[test]
fn mode_parse_server_flag_implies_http_path() {
    let mode = Mode::parse(vec![
        "--server".into(),
        "http://other:3100".into(),
        "search".into(),
        "foo".into(),
    ])
    .unwrap();
    let invocation = match mode {
        Mode::Cli(b) => *b,
        other => panic!("expected Cli, got {other:?}"),
    };
    assert_eq!(
        invocation.flags.server.as_deref(),
        Some("http://other:3100")
    );
    // --server alone does NOT set force_http; http_trigger() decides.
    assert!(!invocation.flags.force_http);
    assert_eq!(invocation.flags.http_trigger(), Some("--server"));
}

#[test]
fn mode_parse_token_flag_implies_http_path() {
    let mode = Mode::parse(vec!["search".into(), "--token=sekret".into(), "foo".into()]).unwrap();
    let invocation = match mode {
        Mode::Cli(b) => *b,
        other => panic!("expected Cli, got {other:?}"),
    };
    assert_eq!(invocation.flags.token.as_deref(), Some("sekret"));
    assert_eq!(invocation.flags.http_trigger(), Some("--token"));
}

#[test]
fn mode_parse_routes_http_setup_to_cli_arm() {
    // `setup` is recognised both by the dedicated Mode::Setup arm AND by the
    // CLI dispatcher. With global flags present, the dedicated arm is skipped
    // (because it requires `global == default`) and dispatch falls through
    // to the CLI arm — run_cli is then responsible for rejecting it.
    // This test pins that routing so a future refactor doesn't silently
    // swallow --http on `setup`.
    let mode = Mode::parse(vec!["--http".into(), "setup".into(), "check".into()]).expect("parses");
    let invocation = match mode {
        Mode::Cli(b) => *b,
        other => panic!("expected Cli (routed to run_cli for the reject), got {other:?}"),
    };
    assert!(invocation.flags.force_http);
    assert!(matches!(
        invocation.command,
        super::cli::CliCommand::Setup(_)
    ));
}

#[test]
fn mode_parse_rejects_http_flag_on_serve_mcp() {
    let err = Mode::parse(vec!["--http".into(), "serve".into(), "mcp".into()]).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("HTTP flags") && msg.contains("query CLI"),
        "expected guard message, got: {msg}"
    );
}

#[test]
fn mode_parse_rejects_http_flag_on_deploy() {
    let err = Mode::parse(vec!["--http".into(), "deploy".into(), "local".into()]).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Local-only commands") && msg.contains("reject HTTP flags"),
        "expected local-only guard message, got: {msg}"
    );
}

#[test]
fn mode_parse_accepts_new_surface_parity_subcommands() {
    // All five surface-parity subcommands parse at the top level with no
    // flags — `compare`'s required-flag validation lives in
    // `CompareArgs::into_request()`, not the top-level parser, so a bare
    // `compare` is accepted by `Mode::parse` even though running it would
    // later bail.
    for cmd in &["silent-hosts", "clock-skew", "anomalies", "compare", "apps"] {
        let result = Mode::parse(vec![(*cmd).to_string()]);
        assert!(result.is_ok(), "Mode::parse rejected '{cmd}': {result:?}");
    }
}

fn cli_invocation(args: &[&str]) -> super::CliInvocation {
    let mode = Mode::parse(args.iter().map(|arg| (*arg).to_string()).collect()).unwrap();
    let Mode::Cli(invocation) = mode else {
        panic!("expected CLI mode");
    };
    *invocation
}

#[tokio::test]
async fn run_cli_rejects_http_flags_for_local_only_compose_setup_and_inventory() {
    for (args, expected) in [
        (
            &["--http", "compose", "status"][..],
            "`compose` (local-only command)",
        ),
        (
            &["--server", "http://127.0.0.1:3100", "setup", "check"][..],
            "`setup` (local-only command)",
        ),
        (
            &["inventory", "--token", "secret", "refresh"][..],
            "`inventory` (local-only command)",
        ),
    ] {
        let err = super::run_cli(cli_invocation(args)).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(expected),
            "expected {expected:?} in {msg:?} for args {args:?}"
        );
    }
}

#[tokio::test]
async fn run_cli_rejects_http_flags_for_agent_local_surfaces() {
    for (args, expected) in [
        (
            &["--http", "heartbeat", "agent"][..],
            "--http has no effect on `heartbeat agent`",
        ),
        (
            &[
                "agent-command",
                "wrap",
                "--server",
                "http://127.0.0.1:3100",
                "--spool",
                "/tmp/spool.jsonl",
                "--",
                "true",
            ][..],
            "`agent-command wrap` (wrapper command)",
        ),
        (
            &[
                "shell",
                "index",
                "--path",
                "/tmp/history",
                "--token",
                "secret",
            ][..],
            "local agent commands",
        ),
    ] {
        let err = super::run_cli(cli_invocation(args)).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(expected),
            "expected {expected:?} in {msg:?} for args {args:?}"
        );
    }
}

#[test]
fn colorize_setup_status_renders_all_status_variants() {
    use cortex::setup::SetupStatus;

    assert!(super::colorize_setup_status(&SetupStatus::Ok).contains("Ok"));
    assert!(super::colorize_setup_status(&SetupStatus::Warn).contains("Warn"));
    assert!(super::colorize_setup_status(&SetupStatus::Error).contains("Error"));
    assert!(super::colorize_setup_status(&SetupStatus::Skipped).contains("Skipped"));
}

// Top-level help banner content + per-command drift coverage now live in
// `src/cli/help_tests.rs` (the `CATALOG` is the source of truth, not a flat
// USAGE const).

#[test]
fn install_color_from_args_parses_and_strips() {
    use cli::color::install_color_from_args;

    let mut a = vec!["--no-color".to_string(), "stats".to_string()];
    install_color_from_args(&mut a).unwrap();
    assert_eq!(a, vec!["stats".to_string()], "--no-color must be stripped");

    let mut b = vec!["--color=never".to_string(), "tail".to_string()];
    install_color_from_args(&mut b).unwrap();
    assert_eq!(
        b,
        vec!["tail".to_string()],
        "--color=VALUE must be stripped"
    );

    let mut c = vec![
        "--color".to_string(),
        "always".to_string(),
        "tail".to_string(),
    ];
    install_color_from_args(&mut c).unwrap();
    assert_eq!(
        c,
        vec!["tail".to_string()],
        "--color VALUE must be stripped"
    );

    let mut d = vec!["--color".to_string(), "search".to_string()];
    install_color_from_args(&mut d).unwrap();
    assert_eq!(
        d,
        vec!["search".to_string()],
        "bare --color (no value word) must be stripped, treating `search` as the command"
    );

    // `--` sentinel: nothing past it is touched.
    let mut e = vec![
        "wrap".to_string(),
        "--".to_string(),
        "--color".to_string(),
        "never".to_string(),
    ];
    install_color_from_args(&mut e).unwrap();
    assert_eq!(
        e,
        vec![
            "wrap".to_string(),
            "--".to_string(),
            "--color".to_string(),
            "never".to_string()
        ],
        "args after `--` must be left untouched"
    );

    // Bad value errors.
    let mut f = vec!["--color=technicolor".to_string()];
    assert!(install_color_from_args(&mut f).is_err());
}
