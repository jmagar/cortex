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
