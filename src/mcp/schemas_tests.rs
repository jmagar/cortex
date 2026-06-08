use super::*;

#[test]
fn tool_definitions_include_expected_public_tools() {
    let tools = tool_definitions();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["cortex"]);

    let action = &tools[0]["inputSchema"]["properties"]["action"];
    assert_eq!(action["type"], "string");
    let actions: Vec<&str> = action["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(actions, super::actions::action_names());
}

#[test]
fn tool_definition_exposes_agent_cost_metadata() {
    let tools = tool_definitions();
    let metadata = tools[0]["x-cortex-action-metadata"].as_array().unwrap();
    assert_eq!(metadata.len(), super::actions::ACTION_SPECS.len());
    assert!(metadata
        .iter()
        .any(|action| { action["name"] == "status" && action["cost"] == "cheap" }));
    assert!(metadata
        .iter()
        .any(|action| { action["name"] == "patterns" && action["cost"] == "expensive" }));
    assert_eq!(
        tools[0]["x-cortex-agent-guidance"]["default_bounds"]["timeline_bucket"],
        "minute"
    );
}

#[test]
fn schema_source_ips_exposes_pagination() {
    let tools = tool_definitions();
    let props = &tools[0]["inputSchema"]["properties"];
    let limit_desc = props["limit"]["description"].as_str().unwrap();
    let offset_desc = props["offset"]["description"].as_str().unwrap();
    assert!(
        limit_desc.contains("source_ips"),
        "limit description must document source_ips page size"
    );
    assert!(
        limit_desc.contains("total"),
        "limit description must mention total count in source_ips response"
    );
    assert!(
        offset_desc.contains("source_ips"),
        "offset description must mention source_ips"
    );
    assert!(
        offset_desc.contains("paginate") || offset_desc.contains("page"),
        "offset description must explain pagination usage"
    );
}

#[test]
fn schema_apps_exposes_pagination_and_total() {
    let tools = tool_definitions();
    let props = &tools[0]["inputSchema"]["properties"];
    let from_desc = props["from"]["description"].as_str().unwrap();
    let to_desc = props["to"]["description"].as_str().unwrap();
    let limit_desc = props["limit"]["description"].as_str().unwrap();
    let offset_desc = props["offset"]["description"].as_str().unwrap();
    assert!(
        from_desc.contains("apps"),
        "from description must include apps action"
    );
    assert!(
        to_desc.contains("apps"),
        "to description must include apps action"
    );
    assert!(
        limit_desc.contains("apps"),
        "limit description must document apps page size"
    );
    assert!(
        limit_desc.contains("total"),
        "limit description must mention total count in apps response"
    );
    assert!(
        offset_desc.contains("apps"),
        "offset description must mention apps"
    );
}

#[test]
fn schema_graph_exposes_lookup_and_neighborhood_arguments() {
    let tools = tool_definitions();
    let props = &tools[0]["inputSchema"]["properties"];
    assert_eq!(tools[0]["inputSchema"]["additionalProperties"], false);
    assert_eq!(props["entity_id"]["minimum"], 1);
    assert_eq!(props["depth"]["minimum"], 1);
    assert_eq!(props["depth"]["maximum"], 3);
    for name in [
        "mode",
        "entity_id",
        "entity_type",
        "key",
        "alias_type",
        "alias_key",
        "depth",
        "evidence_sample_limit",
        "payload_budget",
    ] {
        let desc = props[name]["description"].as_str().unwrap_or("");
        assert!(
            desc.contains("action=graph"),
            "{name} description must document graph usage: {desc}"
        );
    }
    assert!(
        props["limit"]["description"]
            .as_str()
            .unwrap()
            .contains("action=graph"),
        "limit description must document graph caps"
    );
}

#[test]
fn schema_graph_target_constraints_match_runtime_validation() {
    let tools = tool_definitions();
    let constraints = tools[0]["inputSchema"]["allOf"].as_array().unwrap();
    let graph_then = constraints
        .iter()
        .find(|constraint| constraint["if"]["properties"]["action"]["const"] == "graph")
        .map(|constraint| &constraint["then"])
        .unwrap();

    let target_or_evidence = graph_then["oneOf"].as_array().unwrap();
    assert_eq!(target_or_evidence.len(), 2);
    let target_strategies = target_or_evidence[0]["oneOf"].as_array().unwrap();
    assert_eq!(target_strategies.len(), 3);
    assert_eq!(target_strategies[0]["required"][0], "entity_id");
    assert_eq!(target_strategies[1]["required"][0], "entity_type");
    assert_eq!(target_strategies[1]["required"][1], "key");
    assert_eq!(target_strategies[2]["required"][0], "alias_type");
    assert_eq!(target_strategies[2]["required"][1], "alias_key");
    assert_eq!(target_or_evidence[1]["required"][0], "mode");
    assert_eq!(target_or_evidence[1]["required"][1], "evidence_id");

    let depth_constraints = graph_then["allOf"].as_array().unwrap();
    assert!(depth_constraints.iter().any(|constraint| {
        constraint["then"]["properties"]["depth"]["maximum"] == 1
            && constraint["then"]["properties"]["depth"]["minimum"] == 1
    }));
}

#[test]
fn schema_mode_constraints_are_action_specific() {
    let tools = tool_definitions();
    let schema = &tools[0]["inputSchema"];
    let constraints = schema["allOf"].as_array().unwrap();

    let graph_modes = constraints
        .iter()
        .find(|constraint| constraint["if"]["properties"]["action"]["const"] == "graph")
        .and_then(|constraint| constraint["then"]["properties"]["mode"]["enum"].as_array())
        .unwrap();
    assert!(graph_modes.iter().any(|mode| mode == "around"));
    assert!(!graph_modes.iter().any(|mode| mode == "host_services"));

    let map_modes = constraints
        .iter()
        .find(|constraint| constraint["if"]["properties"]["action"]["const"] == "map")
        .and_then(|constraint| constraint["then"]["properties"]["mode"]["enum"].as_array())
        .unwrap();
    assert!(map_modes.iter().any(|mode| mode == "host_services"));
    assert!(!map_modes.iter().any(|mode| mode == "around"));
}

#[test]
fn schema_timeline_and_patterns_warn_on_full_history_scan() {
    let tools = tool_definitions();
    let props = &tools[0]["inputSchema"]["properties"];
    let from_desc = props["from"]["description"].as_str().unwrap();
    assert_eq!(
        props["scan_limit"]["maximum"],
        crate::db::PATTERN_SCAN_LIMIT_MAX
    );
    assert!(props["scan_limit"]["description"]
        .as_str()
        .unwrap()
        .contains("max 10000"));
    assert!(
        from_desc.contains("full-history scan"),
        "from/to description must warn that omitting them causes a full-history scan for timeline/patterns"
    );
}
