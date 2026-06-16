use super::*;

#[test]
fn parse_timeline_collects_bucket_group_and_filters() {
    let args = strings(&[
        "--bucket",
        "hour",
        "--group-by",
        "hostname",
        "--hostname=host1",
        "--json",
    ]);

    let command = parse_timeline(&args).unwrap();

    match command {
        crate::cli::CliCommand::Timeline(args) => {
            assert_eq!(args.bucket.as_deref(), Some("hour"));
            assert_eq!(args.group_by.as_deref(), Some("hostname"));
            assert_eq!(args.hostname.as_deref(), Some("host1"));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_source_ips_accepts_limit_and_offset() {
    let args = strings(&["--limit", "10", "--offset=5"]);

    let command = parse_source_ips(&args).unwrap();

    match command {
        crate::cli::CliCommand::SourceIps(args) => {
            assert_eq!(args.limit, Some(10));
            assert_eq!(args.offset, Some(5));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_errors_accepts_limit_for_bounded_agent_output() {
    let args = strings(&["--from=2026-01-01T00:00:00Z", "--limit", "10", "--json"]);

    let command = parse_errors(&args).unwrap();

    match command {
        crate::cli::CliCommand::Errors(args) => {
            assert_eq!(args.from.as_deref(), Some("2026-01-01T00:00:00+00:00"));
            assert_eq!(args.limit, Some(10));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_filter_collects_structured_filters_and_rejects_query_terms() {
    let args = strings(&[
        "--source-kind=docker-stream",
        "--docker-host",
        "dookie",
        "--container=cortex",
        "--stream=stdout",
        "--event-action",
        "die",
        "--tool=claude",
        "--project=/home/jmagar/workspace/cortex",
        "--session-id=abc123",
        "--limit=25",
        "--json",
    ]);

    let command = parse_filter(&args).unwrap();

    match command {
        crate::cli::CliCommand::Filter(args) => {
            assert_eq!(args.source_kind.as_deref(), Some("docker-stream"));
            assert_eq!(args.docker_host.as_deref(), Some("dookie"));
            assert_eq!(args.container.as_deref(), Some("cortex"));
            assert_eq!(args.stream.as_deref(), Some("stdout"));
            assert_eq!(args.event_action.as_deref(), Some("die"));
            assert_eq!(args.tool.as_deref(), Some("claude"));
            assert_eq!(
                args.project.as_deref(),
                Some("/home/jmagar/workspace/cortex")
            );
            assert_eq!(args.session_id.as_deref(), Some("abc123"));
            assert_eq!(args.limit, Some(25));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let err = parse_filter(&strings(&["error"])).unwrap_err();
    assert!(
        err.to_string()
            .contains("filter does not accept positional query terms")
    );
}

#[test]
fn parse_search_tail_sessions_incident_and_correlate_cover_common_filters() {
    let search = parse_search(&strings(&[
        "--hostname=host1",
        "--source-ip=10.0.0.1",
        "--severity=err",
        "--app-name=cortex",
        "--facility=daemon",
        "--exclude-facility=kern",
        "--from=2026-01-01T00:00:00Z",
        "--to=2026-01-02T00:00:00Z",
        "--received-from=2026-01-03T00:00:00Z",
        "--received-to=2026-01-04T00:00:00Z",
        "--limit=30",
        "--json",
        "disk",
        "full",
    ]))
    .unwrap();
    match search {
        crate::cli::CliCommand::Search(args) => {
            assert_eq!(args.query.as_deref(), Some("disk full"));
            assert_eq!(args.hostname.as_deref(), Some("host1"));
            assert_eq!(
                args.received_to.as_deref(),
                Some("2026-01-04T00:00:00+00:00")
            );
            assert_eq!(args.limit, Some(30));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let tail = parse_tail(&strings(&[
        "--hostname=host1",
        "--source-ip=10.0.0.1",
        "-n",
        "12",
    ]))
    .unwrap();
    match tail {
        crate::cli::CliCommand::Tail(args) => {
            assert_eq!(args.hostname.as_deref(), Some("host1"));
            assert_eq!(args.n, Some(12));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let sessions = parse_sessions(&strings(&[
        "--project=/repo",
        "--tool=Bash",
        "--hostname=host1",
        "--from=2026-01-01T00:00:00Z",
        "--to=2026-01-02T00:00:00Z",
        "--limit=4",
    ]))
    .unwrap();
    match sessions {
        crate::cli::CliCommand::Sessions(args) => {
            assert_eq!(args.project.as_deref(), Some("/repo"));
            assert_eq!(args.tool.as_deref(), Some("Bash"));
            assert_eq!(args.limit, Some(4));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let incident = parse_incident(&strings(&[
        "--around=t0",
        "--minutes=10",
        "--host=host1",
        "--limit=50",
    ]))
    .unwrap();
    match incident {
        crate::cli::CliCommand::Incident(args) => {
            assert_eq!(args.around, "t0");
            assert_eq!(args.minutes, Some(10));
            assert_eq!(args.hostname.as_deref(), Some("host1"));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let correlate = parse_correlate(&strings(&[
        "--reference-time=t0",
        "--window-minutes=5",
        "--severity-min=warn",
        "--hostname=host1",
        "--source-ip=10.0.0.1",
        "--query=panic",
        "--limit=99",
    ]))
    .unwrap();
    match correlate {
        crate::cli::CliCommand::Correlate(args) => {
            assert_eq!(args.reference_time, "t0");
            assert_eq!(args.window_minutes, Some(5));
            assert_eq!(args.query.as_deref(), Some("panic"));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_log_commands_report_help_and_unknown_argument_errors() {
    for (parser, args, expected) in [
        (
            parse_search as fn(&[String]) -> anyhow::Result<crate::cli::CliCommand>,
            vec!["--help"],
            "use `cortex --help`",
        ),
        (parse_filter, vec!["--bogus"], "unknown filter option"),
        (parse_tail, vec!["--bogus"], "unknown tail option"),
        (
            parse_sessions,
            vec!["extra"],
            "unexpected sessions argument",
        ),
        (parse_incident, vec!["--service=x"], "requires --around"),
        (
            parse_correlate,
            vec!["--query=x"],
            "requires --reference-time",
        ),
        (
            parse_source_ips,
            vec!["--bogus"],
            "unknown source-ips option",
        ),
        (parse_timeline, vec!["--bogus"], "unknown timeline option"),
        (parse_patterns, vec!["--bogus"], "unknown patterns option"),
        (
            parse_ingest_rate,
            vec!["--host"],
            "unknown ingest-rate option",
        ),
    ] {
        let err = parser(&strings(&args)).unwrap_err().to_string();
        assert!(err.contains(expected), "expected {expected:?}, got {err:?}");
    }
}

#[test]
fn parse_patterns_accepts_limit_alias_for_top_n() {
    let args = strings(&["--limit=7"]);

    let command = parse_patterns(&args).unwrap();

    match command {
        crate::cli::CliCommand::Patterns(args) => {
            assert_eq!(args.top_n, Some(7));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn search_normalizes_relative_from() {
    let cmd = parse_search(&strings(&["error", "--from", "1h"])).unwrap();
    let crate::cli::CliCommand::Search(args) = cmd else {
        panic!("expected Search");
    };
    let from = args.from.expect("from set");
    // Relative input is normalized to an absolute RFC3339 timestamp at parse time.
    assert!(
        from.contains('T') && from.ends_with("+00:00"),
        "expected normalized RFC3339, got {from}"
    );
}
