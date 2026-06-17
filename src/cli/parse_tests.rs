use super::super::{
    AiCommand, FileTailAddArgs, FileTailCommand, FileTailListArgs, HeartbeatAgentArgs,
    HeartbeatCommand, InventoryArgs, InventoryCommand, OutputArgs,
};
use super::*;

#[test]
fn parse_routes_stats() {
    assert_eq!(
        parse_command(vec!["stats".to_string()]).unwrap(),
        CliCommand::Stats(OutputArgs::default())
    );
}

#[test]
fn parses_file_tail_add() {
    let command = parse_command(vec![
        "file-tail".into(),
        "add".into(),
        "--id".into(),
        "swag-access".into(),
        "--path".into(),
        "/mnt/appdata/swag/log/nginx/access.log".into(),
        "--tag".into(),
        "swag-access".into(),
        "--host".into(),
        "squirts".into(),
        "--facility".into(),
        "local4".into(),
        "--severity".into(),
        "info".into(),
        "--from-start".into(),
        "--json".into(),
    ])
    .unwrap();

    assert_eq!(
        command,
        CliCommand::FileTail(FileTailCommand::Add(FileTailAddArgs {
            id: "swag-access".into(),
            path: "/mnt/appdata/swag/log/nginx/access.log".into(),
            tag: "swag-access".into(),
            host: Some("squirts".into()),
            facility: Some("local4".into()),
            severity: Some("info".into()),
            start_at_end: false,
            json: true,
        }))
    );
}

#[test]
fn file_tail_add_requires_hostname() {
    let err = parse_command(vec![
        "file-tail".into(),
        "add".into(),
        "--id".into(),
        "swag-access".into(),
        "--path".into(),
        "/mnt/appdata/swag/log/nginx/access.log".into(),
        "--tag".into(),
        "swag-access".into(),
    ])
    .unwrap_err();

    assert!(err.to_string().contains("--host"));
}

#[test]
fn parses_file_tail_list() {
    let command = parse_command(vec!["file-tail".into(), "list".into(), "--json".into()]).unwrap();
    assert_eq!(
        command,
        CliCommand::FileTail(FileTailCommand::List(FileTailListArgs { json: true }))
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
            docker: false,
            docker_url: None,
            journald: false,
            syslog_target: None,
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
            docker: false,
            docker_url: None,
            journald: false,
            syslog_target: None,
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

#[test]
fn parse_routes_inventory_refresh_json() {
    assert_eq!(
        parse_command(vec![
            "inventory".to_string(),
            "refresh".to_string(),
            "--json".to_string(),
        ])
        .unwrap(),
        CliCommand::Inventory(InventoryCommand::Refresh(InventoryArgs { json: true }))
    );
}

#[test]
fn parse_inventory_requires_subcommand() {
    let err = parse_command(vec!["inventory".to_string()])
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("inventory subcommand is required"),
        "got: {err}"
    );
}

#[test]
fn parse_inventory_unknown_subcommand_suggests() {
    let err = parse_command(vec!["inventory".to_string(), "stats".to_string()])
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("unknown inventory subcommand: stats"),
        "got: {err}"
    );
    assert!(
        err.contains("refresh") || err.contains("status"),
        "got: {err}"
    );
}

#[test]
fn parse_inventory_rejects_unknown_flag() {
    let err = parse_command(vec![
        "inventory".to_string(),
        "refresh".to_string(),
        "--wat".to_string(),
    ])
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("unknown inventory option: --wat"),
        "got: {err}"
    );
}

#[test]
fn parse_inventory_help_does_not_execute_subcommand() {
    let err = parse_command(vec!["inventory".to_string(), "--help".to_string()])
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Usage: cortex inventory refresh"),
        "got: {err}"
    );

    let err = parse_command(vec![
        "inventory".to_string(),
        "refresh".to_string(),
        "--help".to_string(),
    ])
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("Usage: cortex inventory refresh"),
        "got: {err}"
    );
}

#[test]
fn parse_unknown_command_suggests_close_match() {
    let err = parse_command(vec!["serach".to_string()])
        .unwrap_err()
        .to_string();

    assert!(err.contains("Did you mean `search`?"), "got: {err}");
}

// ─── Heartbeat fleet state parity (cxih.4) ──────────────────────────────────

#[test]
fn parse_routes_host_state() {
    assert!(matches!(
        parse_command(vec![
            "host-state".to_string(),
            "--host".to_string(),
            "tootie".to_string(),
            "--json".to_string(),
        ])
        .unwrap(),
        CliCommand::HostState(_)
    ));
}

#[test]
fn parse_host_state_binds_bare_positional_to_host() {
    let cmd = parse_command(vec!["host-state".to_string(), "dookie".to_string()]).unwrap();
    let CliCommand::HostState(args) = cmd else {
        panic!("expected HostState")
    };
    assert_eq!(args.host.as_deref(), Some("dookie"));
}

#[test]
fn parse_host_state_requires_host_selector_with_usage() {
    let err = parse_command(vec!["host-state".to_string()])
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("requires --host-id ID or --host HOST"),
        "got: {err}"
    );
    assert!(err.contains("Usage: cortex host-state"), "got: {err}");
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
fn parse_routes_graph_evidence() {
    let command = parse_command(vec![
        "graph".to_string(),
        "evidence".to_string(),
        "123".to_string(),
        "--payload-budget=8192".to_string(),
        "--json".to_string(),
    ])
    .unwrap();
    match command {
        CliCommand::Graph(crate::cli::GraphCommand::Evidence(args)) => {
            assert_eq!(args.evidence_id, 123);
            assert_eq!(args.payload_budget, Some(8192));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_graph_evidence_rejects_missing_non_integer_and_extra_args() {
    let missing = parse_command(vec!["graph".to_string(), "evidence".to_string()])
        .unwrap_err()
        .to_string();
    assert!(missing.contains("requires <evidence-id>"), "got: {missing}");

    let non_integer = parse_command(vec![
        "graph".to_string(),
        "evidence".to_string(),
        "nope".to_string(),
    ])
    .unwrap_err()
    .to_string();
    assert!(
        non_integer.contains("must be an integer"),
        "got: {non_integer}"
    );

    let extra = parse_command(vec![
        "graph".to_string(),
        "evidence".to_string(),
        "123".to_string(),
        "extra".to_string(),
    ])
    .unwrap_err()
    .to_string();
    assert!(extra.contains("exactly one"), "got: {extra}");
}

#[test]
fn parse_routes_graph_status_and_rebuild() {
    assert!(matches!(
        parse_command(vec!["graph".to_string(), "status".to_string()]).unwrap(),
        CliCommand::Graph(crate::cli::GraphCommand::Status(_))
    ));
    assert!(matches!(
        parse_command(vec![
            "graph".to_string(),
            "rebuild".to_string(),
            "--json".to_string()
        ])
        .unwrap(),
        CliCommand::Graph(crate::cli::GraphCommand::Rebuild(_))
    ));
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

// Regression: every CLI flag whose value is bound into a SQL timestamp
// comparison must route through the shared time parser, so relative/keyword
// input is normalized to RFC3339 (and non-time input is rejected) rather than
// stored raw and compared lexically — a silent-failure source. parse_logs's
// search/filter/tail/errors/timeline/patterns/incident/correlate are covered
// elsewhere; this pins the previously-unnormalized commands.
#[test]
fn time_flags_normalize_relative_across_state_admin_and_ai_commands() {
    // apps --since/--until
    let CliCommand::Apps(a) =
        parse_command(vec!["apps".into(), "--since".into(), "1h".into()]).unwrap()
    else {
        panic!("expected Apps")
    };
    let s = a.since.expect("apps since");
    assert!(s.ends_with("+00:00"), "apps --since not normalized: {s}");

    // clock-skew --since
    let CliCommand::ClockSkew(c) =
        parse_command(vec!["clock-skew".into(), "--since".into(), "2d".into()]).unwrap()
    else {
        panic!("expected ClockSkew")
    };
    assert!(c.since.unwrap().ends_with("+00:00"));

    // compare --a-from (and the other three share the code path)
    let CliCommand::Compare(cmp) =
        parse_command(vec!["compare".into(), "--a-from".into(), "1h".into()]).unwrap()
    else {
        panic!("expected Compare")
    };
    assert!(cmp.a_from.unwrap().ends_with("+00:00"));

    // correlate-state --reference-time
    let CliCommand::CorrelateState(cs) = parse_command(vec![
        "correlate-state".into(),
        "--reference-time".into(),
        "1h".into(),
    ])
    .unwrap() else {
        panic!("expected CorrelateState")
    };
    assert!(cs.reference_time.unwrap().ends_with("+00:00"));

    // host-state (bare positional host) --since
    let CliCommand::HostState(hs) = parse_command(vec![
        "host-state".into(),
        "dookie".into(),
        "--since".into(),
        "30m".into(),
    ])
    .unwrap() else {
        panic!("expected HostState")
    };
    assert!(hs.since.unwrap().ends_with("+00:00"));

    // ai search --since
    let CliCommand::Ai(AiCommand::Search(ai)) = parse_command(vec![
        "ai".into(),
        "search".into(),
        "boom".into(),
        "--since".into(),
        "1h".into(),
    ])
    .unwrap() else {
        panic!("expected Ai Search")
    };
    assert!(ai.since.unwrap().ends_with("+00:00"));
}

#[test]
fn time_flags_reject_non_time_values() {
    for cmd in [
        vec!["apps".to_string(), "--since".into(), "notatime".into()],
        vec![
            "clock-skew".to_string(),
            "--since".into(),
            "notatime".into(),
        ],
        vec!["compare".to_string(), "--a-from".into(), "notatime".into()],
        vec![
            "correlate-state".to_string(),
            "--reference-time".into(),
            "notatime".into(),
        ],
        vec![
            "host-state".to_string(),
            "dookie".into(),
            "--since".into(),
            "notatime".into(),
        ],
        vec![
            "ai".to_string(),
            "search".into(),
            "q".into(),
            "--since".into(),
            "notatime".into(),
        ],
    ] {
        assert!(
            parse_command(cmd.clone()).is_err(),
            "expected error for {cmd:?}"
        );
    }
}
