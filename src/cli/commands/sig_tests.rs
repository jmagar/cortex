use super::*;

#[test]
fn parse_sig_ack_collects_notes_and_json() {
    let args = strings(&["ack", "hash1", "--notes", "fixed", "--json"]);

    let command = parse_sig(&args).unwrap();

    match command {
        CliCommand::Sig(SigCommand::Ack(args)) => {
            assert_eq!(args.signature_hash, "hash1");
            assert_eq!(args.notes.as_deref(), Some("fixed"));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
