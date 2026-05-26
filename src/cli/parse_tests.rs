use super::super::{HeartbeatAgentArgs, HeartbeatCommand, OutputArgs};
use super::*;

#[test]
fn parse_routes_stats() {
    assert_eq!(
        parse_command(vec!["stats".to_string()]).unwrap(),
        CliCommand::Stats(OutputArgs::default())
    );
}

#[test]
fn parse_routes_heartbeat_agent_defaults() {
    assert_eq!(
        parse_command(vec!["heartbeat".to_string(), "agent".to_string()]).unwrap(),
        CliCommand::Heartbeat(HeartbeatCommand::Agent(HeartbeatAgentArgs {
            target: None,
            token: None,
            interval_secs: 30,
            probe_deadline_ms: 2000,
            collection_deadline_ms: 5000,
            retry_buffer: 32,
            once: false,
            emit: false,
            json: false,
            host_id_path: None,
        }))
    );
}

#[test]
fn parse_routes_heartbeat_agent_flags() {
    assert_eq!(
        parse_command(vec![
            "heartbeat".to_string(),
            "agent".to_string(),
            "--target".to_string(),
            "http://127.0.0.1:3100".to_string(),
            "--token".to_string(),
            "secret".to_string(),
            "--interval-secs".to_string(),
            "15".to_string(),
            "--probe-deadline-ms".to_string(),
            "100".to_string(),
            "--collection-deadline-ms".to_string(),
            "300".to_string(),
            "--retry-buffer".to_string(),
            "4".to_string(),
            "--host-id-path".to_string(),
            "/tmp/host-id".to_string(),
            "--once".to_string(),
            "--json".to_string(),
        ])
        .unwrap(),
        CliCommand::Heartbeat(HeartbeatCommand::Agent(HeartbeatAgentArgs {
            target: Some("http://127.0.0.1:3100".to_string()),
            token: Some("secret".to_string()),
            interval_secs: 15,
            probe_deadline_ms: 100,
            collection_deadline_ms: 300,
            retry_buffer: 4,
            once: true,
            emit: false,
            json: true,
            host_id_path: Some("/tmp/host-id".to_string()),
        }))
    );
}

#[test]
fn parse_rejects_missing_command() {
    let err = parse_command(Vec::new()).unwrap_err().to_string();

    assert!(err.contains("CLI command is required"));
}

#[test]
fn parse_rejects_unknown_command() {
    let err = parse_command(vec!["wat".to_string()])
        .unwrap_err()
        .to_string();

    assert!(err.contains("unknown CLI command: wat"));
}
