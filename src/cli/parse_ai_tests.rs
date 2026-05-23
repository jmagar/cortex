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

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
