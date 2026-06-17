use super::*;

#[test]
fn parse_notify_recent_normalizes_since_and_accepts_negative_limit() {
    // `--since` is bound into `fired_at >= ?` so it must normalize to RFC3339
    // (a relative `1h` becomes an absolute timestamp). A negative `--limit`
    // parses here and is range-checked later at dispatch.
    let args = strings(&["recent", "--since", "1h", "--limit", "-1", "--json"]);

    let command = parse_notify(&args).unwrap();

    match command {
        CliCommand::Notify(NotifyCommand::Recent(args)) => {
            assert!(
                args.since.as_deref().unwrap().ends_with("+00:00"),
                "since not normalized: {:?}",
                args.since
            );
            assert_eq!(args.limit, Some(-1));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_notify_recent_rejects_non_time_since() {
    let err = parse_notify(&strings(&["recent", "--since", "-3600"]))
        .unwrap_err()
        .to_string();
    // Must fail specifically on the time value, not some unrelated parser error.
    assert!(
        err.contains("time") || err.contains("--since"),
        "expected a time-specific error, got: {err}"
    );
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
