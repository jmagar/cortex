use super::*;

#[test]
fn palette_constants_match_documented_ansi_256_values() {
    assert_eq!(SERVICE_NAME, 211);
    assert_eq!(ACCENT_PRIMARY, 39);
    assert_eq!(TEXT_MUTED, 250);
    assert_eq!(SUCCESS, 115);
    assert_eq!(WARN, 180);
    assert_eq!(ERROR, 174);
}

#[test]
fn bold_wraps_text_with_bold_ansi_256_foreground_and_reset() {
    assert_eq!(
        bold(ACCENT_PRIMARY, "cortex"),
        "\x1b[1;38;5;39mcortex\x1b[0m"
    );
}

#[test]
fn paint_wraps_text_with_plain_ansi_256_foreground_and_reset() {
    assert_eq!(paint(WARN, "warn"), "\x1b[38;5;180mwarn\x1b[0m");
}

#[test]
fn dim_wraps_text_with_dim_and_reset() {
    assert_eq!(dim("metadata"), "\x1b[2mmetadata\x1b[0m");
}
