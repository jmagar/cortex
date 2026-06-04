use super::*;

#[test]
fn truncate_text_respects_utf8_boundary() {
    let (out, truncated) = truncate_text("hello😀world", 6);

    assert!(truncated);
    assert_eq!(out, "hello");
    assert!(std::str::from_utf8(out.as_bytes()).is_ok());
}

#[test]
fn truncate_text_reports_untruncated_input() {
    let (out, truncated) = truncate_text("hello", 64);

    assert!(!truncated);
    assert_eq!(out, "hello");
}
