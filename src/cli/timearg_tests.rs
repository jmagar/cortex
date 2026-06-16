use super::*;
use chrono::{TimeZone, Utc};

fn fixed_now() -> chrono::DateTime<Utc> {
    // 2026-06-15T12:00:00Z
    Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap()
}

#[test]
fn parses_relative_durations_back_from_now() {
    let now = fixed_now();
    assert_eq!(
        parse_time_arg("1h", now).unwrap(),
        "2026-06-15T11:00:00+00:00"
    );
    assert_eq!(
        parse_time_arg("30m", now).unwrap(),
        "2026-06-15T11:30:00+00:00"
    );
    assert_eq!(
        parse_time_arg("2d", now).unwrap(),
        "2026-06-13T12:00:00+00:00"
    );
    assert_eq!(
        parse_time_arg("90s", now).unwrap(),
        "2026-06-15T11:58:30+00:00"
    );
}

#[test]
fn rejects_unknown_relative_unit() {
    let now = fixed_now();
    let err = parse_time_arg("5w", now).unwrap_err().to_string();
    assert!(err.contains("time"), "error should mention time: {err}");
}

#[test]
fn parses_keywords() {
    let now = fixed_now();
    assert_eq!(
        parse_time_arg("now", now).unwrap(),
        "2026-06-15T12:00:00+00:00"
    );
    // `today` = midnight UTC of the current day
    assert_eq!(
        parse_time_arg("today", now).unwrap(),
        "2026-06-15T00:00:00+00:00"
    );
    // `yesterday` = midnight UTC of the previous day
    assert_eq!(
        parse_time_arg("yesterday", now).unwrap(),
        "2026-06-14T00:00:00+00:00"
    );
}

#[test]
fn parses_absolute_timestamps() {
    let now = fixed_now();
    // Full RFC3339 passes through (normalized to +00:00).
    assert_eq!(
        parse_time_arg("2026-06-01T08:30:00Z", now).unwrap(),
        "2026-06-01T08:30:00+00:00"
    );
    // Date-only → midnight UTC.
    assert_eq!(
        parse_time_arg("2026-06-01", now).unwrap(),
        "2026-06-01T00:00:00+00:00"
    );
    // Date + HH:MM → that minute, UTC.
    assert_eq!(
        parse_time_arg("2026-06-01 08:30", now).unwrap(),
        "2026-06-01T08:30:00+00:00"
    );
}
