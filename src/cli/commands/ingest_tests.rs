use super::*;

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn parse_ingest_shell_agent_inventory_and_file_tail() {
    let shell = parse_ingest(&strings(&["shell", "index", "--path", "/tmp/history"])).unwrap();
    assert!(matches!(
        shell,
        CliCommand::Ingest(IngestCommand::Shell(
            super::super::super::ShellCommand::Index(_)
        ))
    ));

    let agent = parse_ingest(&strings(&[
        "agent-command",
        "ingest-spool",
        "--path",
        "/tmp/spool.jsonl",
        "--json",
    ]))
    .unwrap();
    assert!(matches!(
        agent,
        CliCommand::Ingest(IngestCommand::AgentCommand(
            super::super::super::AgentCommandCommand::IngestSpool(_)
        ))
    ));

    let inventory = parse_ingest(&strings(&["inventory", "status", "--json"])).unwrap();
    assert!(matches!(
        inventory,
        CliCommand::Ingest(IngestCommand::Inventory(
            super::super::super::InventoryCommand::Status(_)
        ))
    ));

    let file_tail = parse_ingest(&strings(&["file-tail", "list", "--json"])).unwrap();
    assert!(matches!(
        file_tail,
        CliCommand::Ingest(IngestCommand::FileTail(
            super::super::super::FileTailCommand::List(_)
        ))
    ));
}

#[test]
fn parse_ingest_syslog_and_docker_read_only_modes() {
    assert!(matches!(
        parse_ingest(&strings(&["syslog", "status", "--json"])).unwrap(),
        CliCommand::Ingest(IngestCommand::SyslogStatus(args)) if args.json
    ));
    assert!(matches!(
        parse_ingest(&strings(&["docker", "status"])).unwrap(),
        CliCommand::Ingest(IngestCommand::DockerStatus(_))
    ));
    assert!(matches!(
        parse_ingest(&strings(&["docker", "sources", "--json"])).unwrap(),
        CliCommand::Ingest(IngestCommand::DockerSources(args)) if args.json
    ));
}

#[test]
fn parse_ingest_syslog_test_is_deferred() {
    let err = parse_ingest(&strings(&["syslog", "test"]))
        .unwrap_err()
        .to_string();

    assert!(err.contains("deferred"), "got: {err}");
}
