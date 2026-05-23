use super::Mode;

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
fn mode_parse_rejects_unknown_commands() {
    let err = Mode::parse(vec!["serve".into(), "http".into()]).unwrap_err();
    assert!(err.to_string().contains("unknown command"));
}

#[test]
fn mode_parse_keeps_runtime_status_mcp_only() {
    let err = Mode::parse(vec!["status".into()]).unwrap_err();
    assert!(err.to_string().contains("unknown command"));
}

#[test]
fn mode_parse_accepts_ai_namespace() {
    assert!(matches!(
        Mode::parse(vec!["ai".into(), "tools".into(), "--json".into()]).unwrap(),
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
fn mode_parse_accepts_ai_index_timer_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "ai-index-timer".into(),
            "install".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
}

#[test]
fn mode_parse_accepts_ai_watch_service_setup_namespace() {
    assert!(matches!(
        Mode::parse(vec![
            "setup".into(),
            "ai-watch-service".into(),
            "install".into(),
            "--json".into()
        ])
        .unwrap(),
        Mode::Setup(_)
    ));
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
    assert!(err
        .to_string()
        .contains("deploy remote accepts exactly one host"));
}

#[test]
fn mode_parse_rejects_duplicate_ai_watch_service_actions() {
    let err = Mode::parse(vec![
        "setup".into(),
        "ai-watch-service".into(),
        "install".into(),
        "remove".into(),
    ])
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("ai-watch-service action specified more than once"));
}

#[test]
fn mode_parse_accepts_binary_doctor() {
    assert!(matches!(
        Mode::parse(vec!["doctor".into(), "binary".into(), "--json".into()]).unwrap(),
        Mode::DoctorBinary(_)
    ));
}

// ─── Bead 0p8r.6: global HTTP flag plumbing ─────────────────────────────────

use serial_test::serial;

/// Restores SYSLOG_API_TOKEN / SYSLOG_USE_HTTP on drop. Tests below use this
/// to assert `--help` / `--version` work without any token in the env.
struct EnvVarGuard {
    name: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn unset(name: &'static str) -> Self {
        let previous = std::env::var(name).ok();
        std::env::remove_var(name);
        Self { name, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var(self.name, v),
            None => std::env::remove_var(self.name),
        }
    }
}

#[test]
#[serial]
fn help_and_version_bypass_mode_resolution_without_token() {
    // Bead .6 contract: --help and --version must NOT touch env / discovery /
    // services. Without SYSLOG_API_TOKEN and SYSLOG_USE_HTTP set, they should
    // still resolve to Help/Version without erroring.
    let _g1 = EnvVarGuard::unset("SYSLOG_API_TOKEN");
    let _g2 = EnvVarGuard::unset("SYSLOG_USE_HTTP");

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
        msg.contains("only apply to CLI query commands"),
        "expected guard message, got: {msg}"
    );
}

#[test]
fn mode_parse_rejects_http_flag_on_deploy() {
    let err = Mode::parse(vec!["--http".into(), "deploy".into(), "local".into()]).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("compose, service, setup, and deploy are local-only"),
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
