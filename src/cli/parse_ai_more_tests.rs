use super::*;

#[test]
fn parse_ai_similar_collects_query_and_filters() {
    let args = strings(&["disk", "full", "--hostname", "host1", "--limit=7", "--json"]);

    let command = parse_ai_similar(&args).unwrap();

    match command {
        crate::cli::CliCommand::Ai(crate::cli::AiCommand::SimilarIncidents(args)) => {
            assert_eq!(args.query, "disk full");
            assert_eq!(args.hostname.as_deref(), Some("host1"));
            assert_eq!(args.limit, Some(7));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_ai_incident_context_requires_from_and_to() {
    let args = strings(&["--from", "2026-01-01T00:00:00Z"]);

    let err = parse_ai_incident_context(&args).unwrap_err().to_string();

    assert!(err.contains("requires --to"));
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

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
