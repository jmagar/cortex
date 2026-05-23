use super::*;

#[test]
fn parse_notify_recent_accepts_negative_since_and_limit() {
    let args = strings(&["recent", "--since", "-3600", "--limit", "-1", "--json"]);

    let command = parse_notify(&args).unwrap();

    match command {
        CliCommand::Notify(NotifyCommand::Recent(args)) => {
            assert_eq!(args.since.as_deref(), Some("-3600"));
            assert_eq!(args.limit, Some(-1));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
