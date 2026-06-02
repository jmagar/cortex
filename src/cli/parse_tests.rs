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

// ─── Heartbeat fleet state parity (cxih.4) ──────────────────────────────────

#[test]
fn parse_routes_host_state() {
    assert!(matches!(
        parse_command(vec![
            "host-state".to_string(),
            "--hostname".to_string(),
            "tootie".to_string(),
            "--json".to_string(),
        ])
        .unwrap(),
        CliCommand::HostState(_)
    ));
}

#[test]
fn parse_routes_fleet_state() {
    assert!(matches!(
        parse_command(vec!["fleet-state".to_string(), "--exclude-ok".to_string()]).unwrap(),
        CliCommand::FleetState(_)
    ));
}

#[test]
fn parse_fleet_state_rejects_bad_sort() {
    let err = parse_command(vec![
        "fleet-state".to_string(),
        "--sort".to_string(),
        "bogus".to_string(),
    ])
    .unwrap_err()
    .to_string();
    assert!(err.contains("--sort must be"), "got: {err}");
}

#[test]
fn parse_routes_entity_lookup() {
    let command = parse_command(vec![
        "entity".to_string(),
        "host".to_string(),
        "tootie".to_string(),
        "--limit=5".to_string(),
        "--json".to_string(),
    ])
    .unwrap();
    match command {
        CliCommand::Entity(args) => {
            assert_eq!(args.entity_type.as_deref(), Some("host"));
            assert_eq!(args.key.as_deref(), Some("tootie"));
            assert_eq!(args.limit, Some(5));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_routes_entity_alias_lookup() {
    let command = parse_command(vec![
        "entity".to_string(),
        "--alias-type".to_string(),
        "hostname".to_string(),
        "--alias-key".to_string(),
        "tootie".to_string(),
    ])
    .unwrap();
    match command {
        CliCommand::Entity(args) => {
            assert_eq!(args.alias_type.as_deref(), Some("hostname"));
            assert_eq!(args.alias_key.as_deref(), Some("tootie"));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_routes_graph_around_type_key() {
    let command = parse_command(vec![
        "graph".to_string(),
        "around".to_string(),
        "host:tootie".to_string(),
        "--depth".to_string(),
        "1".to_string(),
        "--evidence-sample-limit=2".to_string(),
        "--payload-budget".to_string(),
        "8192".to_string(),
        "--json".to_string(),
    ])
    .unwrap();
    match command {
        CliCommand::Graph(crate::cli::GraphCommand::Around(args)) => {
            assert_eq!(args.entity_type.as_deref(), Some("host"));
            assert_eq!(args.key.as_deref(), Some("tootie"));
            assert_eq!(args.depth, Some(1));
            assert_eq!(args.evidence_sample_limit, Some(2));
            assert_eq!(args.payload_budget, Some(8192));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_routes_graph_explain_type_key() {
    let command = parse_command(vec![
        "graph".to_string(),
        "explain".to_string(),
        "host:tootie".to_string(),
        "--depth".to_string(),
        "3".to_string(),
        "--beam-width=12".to_string(),
        "--max-chains".to_string(),
        "50".to_string(),
        "--evidence-sample-limit=2".to_string(),
        "--payload-budget".to_string(),
        "8192".to_string(),
        "--json".to_string(),
    ])
    .unwrap();
    match command {
        CliCommand::Graph(crate::cli::GraphCommand::Explain(args)) => {
            assert_eq!(args.entity_type.as_deref(), Some("host"));
            assert_eq!(args.key.as_deref(), Some("tootie"));
            assert_eq!(args.depth, Some(3));
            assert_eq!(args.beam_width, Some(12));
            assert_eq!(args.max_chains, Some(50));
            assert_eq!(args.evidence_sample_limit, Some(2));
            assert_eq!(args.payload_budget, Some(8192));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_graph_explain_rejects_bad_depth() {
    let err = parse_command(vec![
        "graph".to_string(),
        "explain".to_string(),
        "host".to_string(),
        "tootie".to_string(),
        "--depth".to_string(),
        "nope".to_string(),
    ])
    .unwrap_err()
    .to_string();
    assert!(err.contains("--depth must be"), "got: {err}");
}

#[test]
fn parse_graph_around_rejects_bad_entity_type() {
    let err = parse_command(vec![
        "graph".to_string(),
        "around".to_string(),
        "bogus".to_string(),
        "tootie".to_string(),
    ])
    .unwrap_err()
    .to_string();
    assert!(err.contains("unsupported graph entity type"), "got: {err}");
}

#[test]
fn parse_graph_around_rejects_bad_depth() {
    let err = parse_command(vec![
        "graph".to_string(),
        "around".to_string(),
        "host".to_string(),
        "tootie".to_string(),
        "--depth".to_string(),
        "nope".to_string(),
    ])
    .unwrap_err()
    .to_string();
    assert!(err.contains("--depth must be"), "got: {err}");
}

#[test]
fn parse_routes_correlate_state() {
    assert!(matches!(
        parse_command(vec![
            "correlate-state".to_string(),
            "--reference-time".to_string(),
            "2026-05-25T00:00:00Z".to_string(),
        ])
        .unwrap(),
        CliCommand::CorrelateState(_)
    ));
}

#[test]
fn parse_correlate_state_rejects_unknown_flag() {
    let err = parse_command(vec!["correlate-state".to_string(), "--bogus".to_string()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown correlate-state option"), "got: {err}");
}
