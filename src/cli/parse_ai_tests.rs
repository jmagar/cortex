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
fn parse_ai_unknown_subcommand_suggests_close_match() {
    let err = parse_ai(&strings(&["serach", "error"]))
        .unwrap_err()
        .to_string();

    assert!(err.contains("Did you mean `search`?"), "got: {err}");
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
