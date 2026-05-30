use super::panel_plain;

#[test]
fn panel_renders_title_and_rows() {
    let out = panel_plain(
        "DB Status",
        &[("db_path", "/data/cortex.db"), ("pages", "42")],
    );
    assert!(out.contains("DB Status"));
    assert!(out.contains("db_path"));
    assert!(out.contains("/data/cortex.db"));
    assert!(out.contains("pages"));
    assert!(out.contains("42"));
}

#[test]
fn panel_empty_rows() {
    let out = panel_plain("Empty", &[]);
    assert!(out.contains("Empty"));
    assert!(out.contains("╭"));
    assert!(out.contains("╯"));
}

#[test]
fn panel_rows_have_equal_visible_width() {
    let rows = &[("key", "value"), ("longer_key", "v")];
    let out = panel_plain("Title", rows);
    // Every content line between top/bottom should have identical byte-length
    // when color is disabled (no ANSI codes).
    let lines: Vec<&str> = out.lines().collect();
    let inner: Vec<&str> = lines[1..lines.len() - 1].to_vec();
    let widths: Vec<usize> = inner.iter().map(|l| l.chars().count()).collect();
    assert!(
        widths.windows(2).all(|w| w[0] == w[1]),
        "rows differ: {widths:?}"
    );
}
