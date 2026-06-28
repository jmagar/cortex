use super::*;

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn parse_alerts_signatures_list() {
    let args = strings(&[
        "signatures",
        "list",
        "--include-acknowledged",
        "--limit",
        "20",
        "--json",
    ]);

    let command = parse_alerts(&args).unwrap();

    match command {
        CliCommand::Alerts(AlertsCommand::Signatures(SigCommand::List(args))) => {
            assert_eq!(args.limit, Some(20));
            assert!(args.include_acknowledged);
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_alerts_signatures_ack_and_unack() {
    let ack = parse_alerts(&strings(&[
        "signatures",
        "ack",
        "hash1",
        "--notes",
        "handled",
        "--json",
    ]))
    .unwrap();
    match ack {
        CliCommand::Alerts(AlertsCommand::Signatures(SigCommand::Ack(args))) => {
            assert_eq!(args.signature_hash, "hash1");
            assert_eq!(args.notes.as_deref(), Some("handled"));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let unack = parse_alerts(&strings(&[
        "signatures",
        "unack",
        "hash1",
        "--reason",
        "regressed",
    ]))
    .unwrap();
    match unack {
        CliCommand::Alerts(AlertsCommand::Signatures(SigCommand::Unack(args))) => {
            assert_eq!(args.signature_hash, "hash1");
            assert_eq!(args.reason.as_deref(), Some("regressed"));
            assert!(!args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_alerts_notifications_recent_and_test() {
    let recent = parse_alerts(&strings(&[
        "notifications",
        "recent",
        "--rule-id",
        "disk",
        "--limit",
        "10",
        "--json",
    ]))
    .unwrap();
    match recent {
        CliCommand::Alerts(AlertsCommand::Notifications(NotifyCommand::Recent(args))) => {
            assert_eq!(args.rule_id.as_deref(), Some("disk"));
            assert_eq!(args.limit, Some(10));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let test = parse_alerts(&strings(&[
        "notifications",
        "test",
        "--body",
        "hello",
        "--json",
    ]))
    .unwrap();
    match test {
        CliCommand::Alerts(AlertsCommand::Notifications(NotifyCommand::Test(args))) => {
            assert_eq!(args.body.as_deref(), Some("hello"));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}
