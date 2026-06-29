//! Tests for the analytics dispatch argument mappers.
//!
//! Each `*Args::into_request()` is exercised to confirm CLI flags map onto the
//! request fields the query surface expects. Split out of
//! `gap_tests` alongside the analytics commands themselves.

use super::*;

#[test]
fn basic_surface_args_map_to_requests() {
    assert_eq!(
        SilentHostsArgs {
            silent_minutes: Some(15),
            json: true,
        }
        .into_request()
        .silent_minutes,
        Some(15)
    );

    let clock = ClockSkewArgs {
        since: Some("2026-06-13T00:00:00Z".to_string()),
        limit: Some(10),
        json: false,
    }
    .into_request();
    assert_eq!(clock.since.as_deref(), Some("2026-06-13T00:00:00Z"));
    assert_eq!(clock.limit, Some(10));

    let anomalies = AnomaliesArgs {
        recent_minutes: Some(30),
        baseline_minutes: Some(120),
        json: false,
    }
    .into_request();
    assert_eq!(anomalies.recent_minutes, Some(30));
    assert_eq!(anomalies.baseline_minutes, Some(120));

    let apps = AppsArgs {
        host: Some("host-a".to_string()),
        since: Some("from".to_string()),
        until: Some("to".to_string()),
        limit: Some(50),
        offset: Some(10),
        json: true,
    }
    .into_request();
    assert_eq!(apps.host.as_deref(), Some("host-a"));
    assert_eq!(apps.since.as_deref(), Some("from"));
    assert_eq!(apps.until.as_deref(), Some("to"));
    assert_eq!(apps.limit, Some(50));
    assert_eq!(apps.offset, Some(10));
}

#[test]
fn compare_requires_reference_fields() {
    let compare = CompareArgs {
        a_from: Some("a1".to_string()),
        a_to: Some("a2".to_string()),
        b_from: Some("b1".to_string()),
        b_to: Some("b2".to_string()),
        json: false,
    }
    .into_request()
    .unwrap();
    assert_eq!(compare.a_from, "a1");
    assert_eq!(compare.b_to, "b2");

    assert!(CompareArgs::default().into_request().is_err());
}
