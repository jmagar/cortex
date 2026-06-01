use super::*;

#[test]
fn strip_trailing_mcp_path_drops_slash_and_mcp() {
    assert_eq!(
        strip_trailing_mcp_path("https://cortex.example/mcp"),
        "https://cortex.example"
    );
    assert_eq!(
        strip_trailing_mcp_path("https://cortex.example/mcp/"),
        "https://cortex.example"
    );
    assert_eq!(
        strip_trailing_mcp_path("https://cortex.example/"),
        "https://cortex.example"
    );
    assert_eq!(
        strip_trailing_mcp_path("https://cortex.example"),
        "https://cortex.example"
    );
}

#[test]
fn append_csv_unique_appends_and_dedupes() {
    assert_eq!(append_csv_unique("", "a"), "a");
    assert_eq!(append_csv_unique("a", "b"), "a,b");
    assert_eq!(append_csv_unique("a,b", "b"), "a,b");
    // empty value is a no-op
    assert_eq!(append_csv_unique("a,b", ""), "a,b");
    // whitespace-trimmed comparison
    assert_eq!(append_csv_unique("a, b", "b"), "a, b");
}

#[test]
fn reject_unsafe_value_errors_on_newline_and_cr() {
    assert!(reject_unsafe_value("X", "ok").is_ok());
    assert!(reject_unsafe_value("X", "bad\nvalue").is_err());
    assert!(reject_unsafe_value("X", "bad\rvalue").is_err());
}
