use super::*;

#[test]
fn suggests_close_command_tokens() {
    assert_eq!(did_you_mean("serach", &["search", "tail"]), Some("search"));
    assert_eq!(
        did_you_mean("--jsoon", &["--json", "--from"]),
        Some("--json")
    );
}

#[test]
fn ignores_distant_tokens() {
    assert_eq!(did_you_mean("xyz", &["search", "compose"]), None);
}

#[test]
fn matches_flag_name_ignoring_equals_value() {
    // Equals-style options must still suggest the right flag — the `=value` part
    // must not pollute the edit distance.
    assert_eq!(
        did_you_mean("--projct=foo", &["--project", "--tool"]),
        Some("--project")
    );
    assert_eq!(
        did_you_mean("--tol=claude", &["--project", "--tool"]),
        Some("--tool")
    );
}
