use super::*;

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn parse_search_collects_query_and_filters() {
    let parsed = CliCommand::parse(strings(&[
        "search",
        "disk",
        "full",
        "--hostname",
        "nas",
        "--source-ip=10.0.0.5:514",
        "--severity",
        "err",
        "--app-name=kernel",
        "--from",
        "2026-01-01T00:00:00Z",
        "--to=2026-01-02T00:00:00Z",
        "--limit",
        "25",
        "--json",
    ]))
    .unwrap();

    assert_eq!(
        parsed,
        CliCommand::Search(SearchArgs {
            query: Some("disk full".into()),
            hostname: Some("nas".into()),
            source_ip: Some("10.0.0.5:514".into()),
            severity: Some("err".into()),
            app_name: Some("kernel".into()),
            from: Some("2026-01-01T00:00:00Z".into()),
            to: Some("2026-01-02T00:00:00Z".into()),
            limit: Some(25),
            json: true,
        })
    );
}

#[test]
fn parse_tail_accepts_positional_count() {
    let parsed = CliCommand::parse(strings(&["tail", "10", "--hostname", "router"])).unwrap();

    assert_eq!(
        parsed,
        CliCommand::Tail(TailArgs {
            n: Some(10),
            hostname: Some("router".into()),
            ..Default::default()
        })
    );
}

#[test]
fn parse_correlate_requires_reference_time() {
    let err = CliCommand::parse(strings(&["correlate", "--limit", "5"])).unwrap_err();

    assert!(err.to_string().contains("reference-time"));
}

#[test]
fn parse_correlate_accepts_reference_time_and_filters() {
    let parsed = CliCommand::parse(strings(&[
        "correlate",
        "--reference-time=2026-01-01T00:00:00Z",
        "--window-minutes",
        "15",
        "--severity-min=warning",
        "--query",
        "timeout",
        "--limit=50",
        "--json",
    ]))
    .unwrap();

    assert_eq!(
        parsed,
        CliCommand::Correlate(CorrelateArgs {
            reference_time: "2026-01-01T00:00:00Z".into(),
            window_minutes: Some(15),
            severity_min: Some("warning".into()),
            query: Some("timeout".into()),
            limit: Some(50),
            json: true,
            ..Default::default()
        })
    );
}

#[test]
fn parse_unknown_option_errors() {
    let err = CliCommand::parse(strings(&["stats", "--bad"])).unwrap_err();

    assert!(err.to_string().contains("unknown stats option"));
}

#[test]
fn parse_ai_search_collects_filters() {
    let parsed = CliCommand::parse(strings(&[
        "ai",
        "search",
        "auth failure",
        "--tool",
        "claude",
        "--project=/tmp/project",
        "--limit",
        "5",
    ]))
    .unwrap();

    assert_eq!(
        parsed,
        CliCommand::Ai(AiCommand::Search(AiSearchArgs {
            query: "auth failure".into(),
            project: Some("/tmp/project".into()),
            tool: Some("claude".into()),
            limit: Some(5),
            ..Default::default()
        }))
    );
}

#[test]
fn parse_ai_abuse_collects_filters_and_context_options() {
    let parsed = CliCommand::parse(strings(&[
        "ai",
        "abuse",
        "--project=/tmp/project",
        "--tool",
        "codex",
        "--limit",
        "5",
        "--before=3",
        "--after",
        "4",
        "--term",
        "dang",
        "--json",
    ]))
    .unwrap();

    assert_eq!(
        parsed,
        CliCommand::Ai(AiCommand::Abuse(AiAbuseArgs {
            project: Some("/tmp/project".into()),
            tool: Some("codex".into()),
            limit: Some(5),
            before: Some(3),
            after: Some(4),
            terms: vec!["dang".into()],
            json: true,
            ..Default::default()
        }))
    );
}

#[test]
fn parse_ai_correlate_collects_cross_reference_filters() {
    let parsed = CliCommand::parse(strings(&[
        "ai",
        "correlate",
        "--project=/tmp/project",
        "--tool",
        "codex",
        "--session-id",
        "sess-1",
        "--ai-query",
        "deploy",
        "--log-query=error",
        "--hostname",
        "host-a",
        "--app-name",
        "dockerd",
        "--window-minutes",
        "15",
        "--severity-min=err",
        "--events-per-anchor",
        "12",
        "--json",
    ]))
    .unwrap();

    assert_eq!(
        parsed,
        CliCommand::Ai(AiCommand::Correlate(AiCorrelateArgs {
            project: Some("/tmp/project".into()),
            tool: Some("codex".into()),
            session_id: Some("sess-1".into()),
            ai_query: Some("deploy".into()),
            log_query: Some("error".into()),
            hostname: Some("host-a".into()),
            app_name: Some("dockerd".into()),
            window_minutes: Some(15),
            severity_min: Some("err".into()),
            events_per_anchor: Some(12),
            json: true,
            ..Default::default()
        }))
    );
}

#[test]
fn parse_ai_context_requires_project() {
    let err = CliCommand::parse(strings(&["ai", "context"])).unwrap_err();
    assert!(err.to_string().contains("requires --project"));
}

#[test]
fn parse_ai_add_requires_file() {
    let err = CliCommand::parse(strings(&["ai", "add"])).unwrap_err();
    assert!(err.to_string().contains("--file"));
}

#[test]
fn parse_ai_watch_defaults() {
    let command = CliCommand::parse(strings(&["ai", "watch"])).unwrap();
    assert_eq!(
        command,
        CliCommand::Ai(AiCommand::Watch(AiWatchArgs {
            path: None,
            debounce_ms: 750,
            settle_ms: 500,
            max_retries: 5,
            no_initial_scan: false,
            json: false,
        }))
    );
}

#[test]
fn parse_ai_watch_all_options() {
    let command = CliCommand::parse(strings(&[
        "ai",
        "watch",
        "--path",
        "/tmp/transcripts",
        "--debounce-ms",
        "100",
        "--settle-ms=250",
        "--max-retries=7",
        "--no-initial-scan",
        "--json",
    ]))
    .unwrap();
    assert_eq!(
        command,
        CliCommand::Ai(AiCommand::Watch(AiWatchArgs {
            path: Some("/tmp/transcripts".into()),
            debounce_ms: 100,
            settle_ms: 250,
            max_retries: 7,
            no_initial_scan: true,
            json: true,
        }))
    );
}

#[test]
fn parse_ai_watch_rejects_zero_timing_values() {
    let err = CliCommand::parse(strings(&["ai", "watch", "--debounce-ms", "0"])).unwrap_err();
    assert!(err.to_string().contains("positive integer"));
}

#[test]
fn parse_ai_index_collects_reindex_controls() {
    let parsed = CliCommand::parse(strings(&[
        "ai",
        "index",
        "--path=/tmp/session.jsonl",
        "--since",
        "2026-05-14T00:00:00Z",
        "--force",
        "--json",
    ]))
    .unwrap();

    assert_eq!(
        parsed,
        CliCommand::Ai(AiCommand::Index(AiIndexArgs {
            path: Some("/tmp/session.jsonl".into()),
            since: Some("2026-05-14T00:00:00Z".into()),
            force: true,
            json: true,
        }))
    );
}

#[test]
fn parse_ai_checkpoints_collects_filters() {
    let parsed = CliCommand::parse(strings(&[
        "ai",
        "checkpoints",
        "--errors",
        "--limit=25",
        "--json",
    ]))
    .unwrap();

    assert_eq!(
        parsed,
        CliCommand::Ai(AiCommand::Checkpoints(AiCheckpointsArgs {
            errors_only: true,
            missing_only: false,
            limit: Some(25),
            json: true,
        }))
    );
}

#[test]
fn parse_ai_errors_collects_limit() {
    let parsed = CliCommand::parse(strings(&["ai", "errors", "--limit", "10", "--json"])).unwrap();

    assert_eq!(
        parsed,
        CliCommand::Ai(AiCommand::Errors(AiErrorsArgs {
            limit: Some(10),
            json: true,
        }))
    );
}

#[test]
fn parse_ai_prune_checkpoints_requires_missing() {
    let err = CliCommand::parse(strings(&["ai", "prune-checkpoints", "--dry-run"])).unwrap_err();
    assert!(err.to_string().contains("--missing"));
}

#[test]
fn parse_ai_doctor_accepts_strict_permissions() {
    let parsed =
        CliCommand::parse(strings(&["ai", "doctor", "--strict-permissions", "--json"])).unwrap();

    assert_eq!(
        parsed,
        CliCommand::Ai(AiCommand::Doctor(AiDoctorArgs {
            json: true,
            strict_permissions: true,
        }))
    );
}

#[test]
fn parse_ai_watch_status_accepts_json() {
    let parsed = CliCommand::parse(strings(&["ai", "watch-status", "--json"])).unwrap();

    assert_eq!(
        parsed,
        CliCommand::Ai(AiCommand::WatchStatus(OutputArgs { json: true }))
    );
}

#[test]
fn parse_ai_smoke_watch_accepts_json() {
    let parsed = CliCommand::parse(strings(&["ai", "smoke-watch", "--json"])).unwrap();

    assert_eq!(
        parsed,
        CliCommand::Ai(AiCommand::SmokeWatch(OutputArgs { json: true }))
    );
}

#[test]
fn smoke_watch_target_uses_codex_root_when_claude_is_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let codex_root = temp.path().join(".codex/sessions");
    std::fs::create_dir_all(&codex_root).unwrap();
    let doctor = AiDoctorReport {
        db_path: "/tmp/syslog.db".into(),
        claude_root: transcript_root_status("/missing", false),
        codex_root: transcript_root_status(&codex_root.to_string_lossy(), true),
        checkpoint_count: 0,
        checkpoint_error_count: 0,
        missing_checkpoint_count: 0,
        imported_record_count: 0,
        parse_error_count: 0,
        newest_indexed_path: None,
        newest_indexed_at: None,
    };

    let target = smoke_watch_target(&doctor, "stamp", "session-1", "2026-05-15T00:00:00Z")
        .expect("codex root should be selected");

    assert_eq!(target.tool, "codex");
    assert_eq!(
        target.project,
        std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string()
    );
    assert!(target.transcript_path.starts_with(codex_root));
    assert!(target.body.contains("\"type\":\"session_meta\""));
    assert!(target.body.contains("\"type\":\"response_item\""));
}

#[test]
fn strict_ai_doctor_permissions_ignore_missing_roots() {
    let doctor = AiDoctorReport {
        db_path: "/tmp/syslog.db".into(),
        claude_root: transcript_root_status("/missing", false),
        codex_root: transcript_root_status("/tmp/codex", true),
        checkpoint_count: 0,
        checkpoint_error_count: 0,
        missing_checkpoint_count: 0,
        imported_record_count: 0,
        parse_error_count: 0,
        newest_indexed_path: None,
        newest_indexed_at: None,
    };

    ensure_ai_doctor_success(&doctor, true).expect("missing roots should not fail strict mode");
}

fn transcript_root_status(
    path: &str,
    available: bool,
) -> syslog_mcp::scanner::TranscriptRootStatus {
    syslog_mcp::scanner::TranscriptRootStatus {
        path: path.to_string(),
        exists: available,
        readable: available,
        writable: available,
        owner_uid: None,
        owner_gid: None,
        mode: None,
        strict_ok: available,
    }
}

#[test]
fn parse_db_status_accepts_json() {
    let parsed = CliCommand::parse(strings(&["db", "status", "--json"])).unwrap();

    assert_eq!(
        parsed,
        CliCommand::Db(DbCommand::Status(OutputArgs { json: true }))
    );
}

#[test]
fn parse_db_checkpoint_accepts_modes() {
    let parsed =
        CliCommand::parse(strings(&["db", "checkpoint", "--mode=truncate", "--json"])).unwrap();

    assert_eq!(
        parsed,
        CliCommand::Db(DbCommand::Checkpoint(DbCheckpointArgs {
            mode: "truncate".into(),
            json: true,
        }))
    );
}

#[test]
fn parse_db_checkpoint_rejects_unknown_mode() {
    let err = CliCommand::parse(strings(&["db", "checkpoint", "--mode", "bogus"])).unwrap_err();
    assert!(err.to_string().contains("passive, full, restart, truncate"));
}

#[test]
fn parse_db_vacuum_and_backup_options() {
    let vacuum = CliCommand::parse(strings(&["db", "vacuum", "--pages", "250"])).unwrap();
    assert_eq!(
        vacuum,
        CliCommand::Db(DbCommand::Vacuum(DbVacuumArgs {
            full: false,
            pages: 250,
            json: false,
        }))
    );

    let backup = CliCommand::parse(strings(&[
        "db",
        "backup",
        "--output=/tmp/syslog-backups",
        "--json",
    ]))
    .unwrap();
    assert_eq!(
        backup,
        CliCommand::Db(DbCommand::Backup(DbBackupArgs {
            output: Some("/tmp/syslog-backups".into()),
            json: true,
        }))
    );
}

#[test]
fn truncate_is_utf8_safe_for_non_ascii_project_names() {
    let value = truncate("项目路径-alpha", 6);
    assert!(value.ends_with('…'));
    assert!(value.is_char_boundary(value.len()));
}

#[test]
fn parse_search_help_points_to_top_level_usage() {
    let err = CliCommand::parse(strings(&["search", "--help"])).unwrap_err();

    assert!(err.to_string().contains("syslog --help"));
}

#[test]
fn parse_compose_status_collects_target() {
    let parsed = CliCommand::parse(strings(&[
        "compose",
        "status",
        "--compose-file",
        "/tmp/docker-compose.yml",
        "--project-name=syslog",
        "--json",
    ]))
    .unwrap();
    match parsed {
        CliCommand::Compose(ComposeCommand::Status(args)) => {
            assert_eq!(
                args.target.compose_file.unwrap(),
                std::path::PathBuf::from("/tmp/docker-compose.yml")
            );
            assert_eq!(args.target.project_name.as_deref(), Some("syslog"));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_compose_upgrade_is_deferred() {
    let err = CliCommand::parse(strings(&["compose", "upgrade"])).unwrap_err();
    assert!(err.to_string().contains("deferred"));
}

#[test]
fn parse_compose_logs_follow_is_deferred() {
    let err = CliCommand::parse(strings(&["compose", "logs", "--follow"])).unwrap_err();
    assert!(err.to_string().contains("deferred"));
}

#[test]
fn parse_compose_down_collects_yes_and_dry_run() {
    let parsed = CliCommand::parse(strings(&["compose", "down", "--yes", "--dry-run"])).unwrap();
    match parsed {
        CliCommand::Compose(ComposeCommand::Down(args)) => {
            assert!(args.options.yes);
            assert!(args.options.dry_run);
            assert!(args.options.non_interactive);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_setup_plugin_hook_collects_json_and_no_repair() {
    let parsed =
        CliCommand::parse(strings(&["setup", "plugin-hook", "--json", "--no-repair"])).unwrap();

    assert_eq!(
        parsed,
        CliCommand::Setup(SetupCommand::PluginHook(PluginHookArgs {
            json: true,
            no_repair: true,
        }))
    );
}

#[test]
fn parse_setup_check_and_repair() {
    assert_eq!(
        CliCommand::parse(strings(&["setup", "check", "--json"])).unwrap(),
        CliCommand::Setup(SetupCommand::Check(SetupArgs { json: true }))
    );
    assert_eq!(
        CliCommand::parse(strings(&["setup", "repair"])).unwrap(),
        CliCommand::Setup(SetupCommand::Repair(SetupArgs { json: false }))
    );
}
