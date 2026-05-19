use super::*;

#[test]
fn tool_definitions_include_expected_public_tools() {
    let tools = tool_definitions();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["syslog"]);

    let action = &tools[0]["inputSchema"]["properties"]["action"];
    assert_eq!(action["type"], "string");
    let actions: Vec<&str> = action["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(actions, SYSLOG_ACTIONS);
}

#[test]
fn schema_source_ips_exposes_limit_with_truncation_doc() {
    let tools = tool_definitions();
    let props = &tools[0]["inputSchema"]["properties"];
    let limit_desc = props["limit"]["description"].as_str().unwrap();
    assert!(
        limit_desc.contains("source_ips"),
        "limit description must document source_ips default and truncated flag"
    );
    assert!(
        limit_desc.contains("truncated"),
        "limit description must mention truncated flag for source_ips"
    );
}

#[test]
fn schema_apps_exposes_from_to_limit() {
    let tools = tool_definitions();
    let props = &tools[0]["inputSchema"]["properties"];
    let from_desc = props["from"]["description"].as_str().unwrap();
    let to_desc = props["to"]["description"].as_str().unwrap();
    let limit_desc = props["limit"]["description"].as_str().unwrap();
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
        "limit description must document apps default and max"
    );
}

#[test]
fn schema_timeline_and_patterns_warn_on_full_history_scan() {
    let tools = tool_definitions();
    let props = &tools[0]["inputSchema"]["properties"];
    let from_desc = props["from"]["description"].as_str().unwrap();
    assert!(
        from_desc.contains("full-history scan"),
        "from/to description must warn that omitting them causes a full-history scan for timeline/patterns"
    );
}
