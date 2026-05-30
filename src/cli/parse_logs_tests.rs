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
            assert_eq!(args.from.as_deref(), Some("2026-01-01T00:00:00Z"));
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
    assert!(err
        .to_string()
        .contains("filter does not accept positional query terms"));
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
