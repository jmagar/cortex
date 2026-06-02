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
