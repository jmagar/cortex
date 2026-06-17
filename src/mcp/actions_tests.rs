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
