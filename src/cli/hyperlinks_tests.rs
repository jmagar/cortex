use super::hyperlink_inner;

#[test]
fn hyperlink_unsupported_returns_label() {
    assert_eq!(
        hyperlink_inner("https://example.com", "click here", false),
        "click here"
    );
}

#[test]
fn hyperlink_unsupported_returns_url_when_label_empty() {
    assert_eq!(
        hyperlink_inner("https://example.com", "", false),
        "https://example.com"
    );
}

#[test]
fn hyperlink_supported_wraps_with_osc8() {
    let out = hyperlink_inner("https://example.com", "click here", true);
    assert!(out.contains("\x1b]8;;https://example.com\x1b\\"));
    assert!(out.contains("click here"));
}

#[test]
fn hyperlink_strips_control_chars() {
    let out = hyperlink_inner("https://ex\x1bample.com", "la\x07bel", false);
    assert_eq!(out, "label");
}
