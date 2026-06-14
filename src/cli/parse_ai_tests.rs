use super::*;

#[test]
fn parse_ai_search_requires_query() {
    let args = strings(&["--project", "/repo"]);

    let err = parse_ai_search(&args).unwrap_err().to_string();

    assert!(err.contains("requires a query"));
}

#[test]
fn parse_ai_watch_rejects_zero_debounce() {
    let args = strings(&["--debounce-ms", "0"]);

    let err = parse_ai_watch(&args).unwrap_err().to_string();

    assert!(err.contains("expects a positive integer"));
}

#[test]
fn parse_ai_blocks_accepts_limit_and_detail() {
    let args = strings(&["--limit", "12", "--detail", "full", "--json"]);

    let command = parse_ai_blocks(&args).unwrap();

    match command {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Blocks(args)) => {
            assert_eq!(args.limit, Some(12));
            assert_eq!(args.detail, crate::cli::AiOutputDetail::Full);
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_dispatches_status_and_smoke_watch_output_flags() {
    let status = parse_ai(&strings(&["watch-status", "--json"])).unwrap();
    match status {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::WatchStatus(args)) => {
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let smoke = parse_ai(&strings(&["smoke-watch", "--json"])).unwrap();
    match smoke {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::SmokeWatch(args)) => {
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_search_abuse_and_correlate_accept_equals_forms() {
    let search = parse_ai_search(&strings(&[
        "--project=/repo",
        "--tool=Edit",
        "--from=2026-01-01T00:00:00Z",
        "--to=2026-01-02T00:00:00Z",
        "--limit=9",
        "--json",
        "disk",
        "full",
    ]))
    .unwrap();
    match search {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Search(args)) => {
            assert_eq!(args.query, "disk full");
            assert_eq!(args.project.as_deref(), Some("/repo"));
            assert_eq!(args.tool.as_deref(), Some("Edit"));
            assert_eq!(args.limit, Some(9));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let abuse = parse_ai_abuse(&strings(&[
        "--project=/repo",
        "--tool=Bash",
        "--from=old",
        "--to=new",
        "--limit=10",
        "--before=2",
        "--after=3",
        "--term=panic",
        "--term",
        "timeout",
    ]))
    .unwrap();
    match abuse {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Abuse(args)) => {
            assert_eq!(args.before, Some(2));
            assert_eq!(args.after, Some(3));
            assert_eq!(args.terms, vec!["panic", "timeout"]);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let correlate = parse_ai_correlate(&strings(&[
        "--project=/repo",
        "--tool=Read",
        "--session-id=s1",
        "--ai-query=build",
        "--log-query=error",
        "--hostname=host1",
        "--source-ip=10.0.0.8",
        "--app-name=cortex",
        "--from=t0",
        "--to=t1",
        "--window-minutes=15",
        "--severity-min=warn",
        "--limit=20",
        "--events-per-anchor=4",
    ]))
    .unwrap();
    match correlate {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Correlate(args)) => {
            assert_eq!(args.session_id.as_deref(), Some("s1"));
            assert_eq!(args.window_minutes, Some(15));
            assert_eq!(args.events_per_anchor, Some(4));
            assert_eq!(args.source_ip.as_deref(), Some("10.0.0.8"));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_inventory_and_indexing_commands_accept_flags() {
    let context =
        parse_ai_context(&strings(&["--project=/repo", "--tool=Edit", "--limit=3"])).unwrap();
    match context {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Context(args)) => {
            assert_eq!(args.project, "/repo");
            assert_eq!(args.tool.as_deref(), Some("Edit"));
            assert_eq!(args.limit, Some(3));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let tools = parse_ai_tools(&strings(&[
        "--project=/repo",
        "--from=a",
        "--to=b",
        "--json",
    ]))
    .unwrap();
    match tools {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Tools(args)) => {
            assert_eq!(args.project.as_deref(), Some("/repo"));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let projects = parse_ai_projects(&strings(&["--tool=Write", "--from=a", "--to=b"])).unwrap();
    match projects {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Projects(args)) => {
            assert_eq!(args.tool.as_deref(), Some("Write"));
            assert_eq!(args.to.as_deref(), Some("b"));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let index = parse_ai_index(&strings(&[
        "--path=/tmp/transcripts",
        "--since=2026",
        "--force",
    ]))
    .unwrap();
    match index {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Index(args)) => {
            assert_eq!(args.path.as_deref(), Some("/tmp/transcripts"));
            assert_eq!(args.since.as_deref(), Some("2026"));
            assert!(args.force);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let add = parse_ai_add(&strings(&[
        "--file=/tmp/session.jsonl",
        "--force",
        "--json",
    ]))
    .unwrap();
    match add {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Add(args)) => {
            assert_eq!(args.file, "/tmp/session.jsonl");
            assert!(args.force);
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_watch_checkpoint_error_and_doctor_commands_accept_flags() {
    let watch = parse_ai_watch(&strings(&[
        "--path=/tmp/sessions",
        "--debounce-ms=25",
        "--settle-ms=30",
        "--max-retries=2",
        "--no-initial-scan",
        "--json",
    ]))
    .unwrap();
    match watch {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Watch(args)) => {
            assert_eq!(args.path.as_deref(), Some("/tmp/sessions"));
            assert_eq!(args.debounce_ms, 25);
            assert_eq!(args.settle_ms, 30);
            assert_eq!(args.max_retries, 2);
            assert!(args.no_initial_scan);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let checkpoints =
        parse_ai_checkpoints(&strings(&["--errors", "--missing", "--limit=11"])).unwrap();
    match checkpoints {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Checkpoints(args)) => {
            assert!(args.errors_only);
            assert!(args.missing_only);
            assert_eq!(args.limit, Some(11));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let errors = parse_ai_errors(&strings(&["--limit=6", "--json"])).unwrap();
    match errors {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Errors(args)) => {
            assert_eq!(args.limit, Some(6));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let prune =
        parse_ai_prune_checkpoints(&strings(&["--missing", "--dry-run", "--limit=2"])).unwrap();
    match prune {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::PruneCheckpoints(args)) => {
            assert!(args.missing_only);
            assert!(args.dry_run);
            assert_eq!(args.limit, Some(2));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let doctor = parse_ai_doctor(&strings(&["--strict-permissions", "--json"])).unwrap();
    match doctor {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Doctor(args)) => {
            assert!(args.strict_permissions);
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_reports_errors_for_missing_required_or_unexpected_args() {
    for (subcommand, parser, args, expected) in [
        (
            "context",
            parse_ai_context as fn(&[String]) -> anyhow::Result<crate::cli::CliCommand>,
            vec!["--tool=Edit"],
            "requires --project",
        ),
        ("add", parse_ai_add, vec!["--json"], "requires --file"),
        (
            "prune",
            parse_ai_prune_checkpoints,
            vec!["--dry-run"],
            "requires --missing",
        ),
    ] {
        let err = parser(&strings(&args)).unwrap_err().to_string();
        assert!(
            err.contains(expected),
            "{subcommand} expected {expected:?}, got {err:?}"
        );
    }

    for (parser, args, expected) in [
        (
            parse_ai_abuse as fn(&[String]) -> anyhow::Result<crate::cli::CliCommand>,
            vec!["extra"],
            "unexpected ai abuse argument",
        ),
        (
            parse_ai_correlate,
            vec!["extra"],
            "unexpected ai correlate argument",
        ),
        (
            parse_ai_context,
            vec!["/repo", "extra"],
            "unexpected ai context argument",
        ),
    ] {
        let err = parser(&strings(&args)).unwrap_err().to_string();
        assert!(err.contains(expected), "got {err:?}");
    }
}

#[test]
fn parse_ai_unknown_subcommand_suggests_close_match() {
    let err = parse_ai(&strings(&["serach", "error"]))
        .unwrap_err()
        .to_string();

    assert!(err.contains("Did you mean `search`?"), "got: {err}");
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
