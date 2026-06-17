use super::*;

#[test]
fn every_action_has_nonempty_description() {
    for spec in ACTION_SPECS {
        assert!(
            !spec.description.is_empty(),
            "{} missing description",
            spec.name
        );
    }
}

#[test]
fn search_action_exposes_common_flags_and_examples() {
    let flags = flags_for("search").expect("search has flags");
    assert!(flags.iter().any(|f| f.flag == "--host"));
    assert!(flags.iter().any(|f| f.flag == "--since"));
    let ex = examples_for("search").expect("search has examples");
    assert!(!ex.is_empty(), "search should ship at least one example");
}

#[test]
fn all_cli_query_actions_have_examples() {
    for name in [
        "search",
        "filter",
        "tail",
        "errors",
        "hosts",
        "apps",
        "timeline",
        "patterns",
        "correlate",
        "source_ips",
        "stats",
        "status",
    ] {
        assert!(
            examples_for(name).map(|e| !e.is_empty()).unwrap_or(false),
            "{name} needs an example"
        );
    }
}

#[test]
fn tail_binds_positional_to_host_and_defaults_limit() {
    assert_eq!(positional_for("tail"), Some("--host"));
    assert_eq!(defaults_for("tail").limit, Some(50));
}

#[test]
fn search_positional_binds_to_query() {
    assert_eq!(positional_for("search"), Some("--query"));
    assert_eq!(defaults_for("search").limit, Some(50));
}

#[test]
fn errors_defaults_to_one_hour_window() {
    assert_eq!(positional_for("errors"), None);
    assert_eq!(defaults_for("errors").since, Some("1h"));
}

#[test]
fn host_state_binds_positional_to_host() {
    assert_eq!(positional_for("host_state"), Some("--host"));
}

#[test]
fn actions_without_positional_metadata_default_to_none() {
    assert_eq!(positional_for("hosts"), None);
    let d = defaults_for("hosts");
    assert_eq!(d.limit, None);
    assert_eq!(d.since, None);
}
