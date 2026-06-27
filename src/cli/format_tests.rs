use super::*;

#[test]
fn truncate_display_text_counts_ellipsis_inside_cap() {
    assert_eq!(truncate_display_text("abcdef", 4), "abc…");
    assert_eq!(truncate_display_text("abcdef", 4).chars().count(), 4);
}

#[test]
fn truncate_display_text_multibyte_safe() {
    assert_eq!(truncate_display_text("éééé", 3), "éé…");
    assert_eq!(truncate_display_text("éééé", 3).chars().count(), 3);
}

#[test]
fn format_duration_ranges() {
    assert_eq!(format_duration(45), "45s");
    assert_eq!(format_duration(90), "1m30s");
    assert_eq!(format_duration(3700), "1h1m");
    assert_eq!(format_duration(90_000), "1d1h");
}
