use super::*;

#[test]
fn parse_ai_similar_collects_query_and_filters() {
    let args = strings(&["disk", "full", "--host", "host1", "--limit=7", "--json"]);

    let command = parse_ai_similar(&args).unwrap();

    match command {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::SimilarIncidents(args)) => {
            assert_eq!(args.query, "disk full");
            assert_eq!(args.host.as_deref(), Some("host1"));
            assert_eq!(args.limit, Some(7));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_similar_and_ask_history_accept_all_filters() {
    let similar = parse_ai_similar(&strings(&[
        "--host=host1",
        "--app=cortex",
        "--severity-min=err",
        "--since=t0",
        "--until=t1",
        "--window-minutes=45",
        "--limit=8",
        "disk",
    ]))
    .unwrap();
    match similar {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::SimilarIncidents(args)) => {
            assert_eq!(args.app.as_deref(), Some("cortex"));
            assert_eq!(args.severity_min.as_deref(), Some("err"));
            assert_eq!(args.window_minutes, Some(45));
            assert_eq!(args.query, "disk");
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let ask = parse_ai_ask_history(&strings(&[
        "--host=host1",
        "--app=cortex",
        "--since=t0",
        "--until=t1",
        "--limit=5",
        "--json",
        "why",
        "failed",
    ]))
    .unwrap();
    match ask {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::AskHistory(args)) => {
            assert_eq!(args.query, "why failed");
            assert_eq!(args.host.as_deref(), Some("host1"));
            assert_eq!(args.limit, Some(5));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_incident_context_requires_from_and_to() {
    let args = strings(&["--since", "2026-01-01T00:00:00Z"]);

    let err = parse_ai_incident_context(&args).unwrap_err().to_string();

    assert!(err.contains("requires --to"));
}

#[test]
fn parse_ai_incident_context_accepts_full_filter_set() {
    let command = parse_ai_incident_context(&strings(&[
        "--since=2026-01-01T00:00:00Z",
        "--until=2026-01-01T00:10:00Z",
        "--host=host1",
        "--app=cortex",
        "--query=panic",
        "--severity-min=warn",
        "--limit=12",
        "--json",
    ]))
    .unwrap();

    match command {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::IncidentContext(args)) => {
            assert_eq!(args.since, "2026-01-01T00:00:00Z");
            assert_eq!(args.until, "2026-01-01T00:10:00Z");
            assert_eq!(args.query.as_deref(), Some("panic"));
            assert_eq!(args.limit, Some(12));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_incidents_accepts_terms_and_window_filters() {
    let command = parse_ai_incidents(&strings(&[
        "--project=/repo",
        "--tool=Bash",
        "--since=t0",
        "--until=t1",
        "--limit=13",
        "--window-minutes=60",
        "--term=panic",
        "--term",
        "timeout",
        "--json",
    ]))
    .unwrap();

    match command {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Incidents(args)) => {
            assert_eq!(args.project.as_deref(), Some("/repo"));
            assert_eq!(args.tool.as_deref(), Some("Bash"));
            assert_eq!(args.window_minutes, Some(60));
            assert_eq!(args.terms, vec!["panic", "timeout"]);
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_investigate_accepts_compact_output_controls() {
    let args = strings(&[
        "--detail=full",
        "--include-transcript",
        "--max-bytes",
        "80",
        "--json",
    ]);

    let command = parse_ai_investigate(&args).unwrap();

    match command {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Investigate(args)) => {
            assert_eq!(args.detail, crate::cli::AiOutputDetail::Full);
            assert!(args.include_transcript);
            assert_eq!(args.max_bytes, Some(80));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_investigate_accepts_incident_filters_and_limits() {
    let command = parse_ai_investigate(&strings(&[
        "--project=/repo",
        "--tool=Edit",
        "--since=t0",
        "--until=t1",
        "--limit=21",
        "--window-minutes=30",
        "--correlation-window-minutes=7",
        "--term=panic",
        "--detail=compact",
        "--max-bytes=1024",
    ]))
    .unwrap();

    match command {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Investigate(args)) => {
            assert_eq!(args.project.as_deref(), Some("/repo"));
            assert_eq!(args.limit, Some(21));
            assert_eq!(args.window_minutes, Some(30));
            assert_eq!(args.correlation_window_minutes, Some(7));
            assert_eq!(args.terms, vec!["panic"]);
            assert_eq!(args.detail, crate::cli::AiOutputDetail::Compact);
            assert_eq!(args.max_bytes, Some(1024));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_assess_accepts_incident_and_investigation_filters() {
    let command = parse_ai_assess(&strings(&[
        "incident-1",
        "--model=gemini-test",
        "--project=/repo",
        "--tool=Bash",
        "--since=t0",
        "--until=t1",
        "--limit=34",
        "--window-minutes=44",
        "--correlation-window-minutes=9",
        "--term=auth",
        "--json",
    ]))
    .unwrap();

    match command {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::Assess(args)) => {
            assert_eq!(args.incident_id, "incident-1");
            assert_eq!(args.model.as_deref(), Some("gemini-test"));
            assert_eq!(args.project.as_deref(), Some("/repo"));
            assert_eq!(args.limit, Some(34));
            assert_eq!(args.window_minutes, Some(44));
            assert_eq!(args.correlation_window_minutes, Some(9));
            assert_eq!(args.terms, vec!["auth"]);
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_more_reports_required_query_and_unexpected_argument_errors() {
    for (parser, args, expected) in [
        (
            parse_ai_similar as fn(&[String]) -> anyhow::Result<crate::cli::CliCommand>,
            vec!["--limit=1"],
            "requires a query",
        ),
        (parse_ai_ask_history, vec!["--limit=1"], "requires a query"),
        (
            parse_ai_incident_context,
            vec!["--since=t0", "--until=t1", "extra"],
            "unexpected positional argument",
        ),
        (
            parse_ai_incidents,
            vec!["extra"],
            "unexpected ai incidents argument",
        ),
        (
            parse_ai_investigate,
            vec!["extra"],
            "unexpected ai investigate argument",
        ),
        (
            parse_ai_assess,
            vec!["id1", "id2"],
            "unexpected extra argument",
        ),
    ] {
        let err = parser(&strings(&args)).unwrap_err().to_string();
        assert!(err.contains(expected), "expected {expected:?}, got {err:?}");
    }

    let err = parse_ai_assess(&[]).unwrap_err().to_string();
    assert!(err.contains("requires an <incident_id>"));
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
