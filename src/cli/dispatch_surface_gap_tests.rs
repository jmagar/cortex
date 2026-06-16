//! Tests for the dispatch surface-gap argument mappers.
//!
//! Each `Cli*Args::into_request()` is exercised to confirm CLI flags map onto
//! the request fields the query surface expects. Extracted from an inline
//! `mod tests` into this sidecar to honour the repo's sidecar-test convention
//! and keep the production module under the size limit.

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
fn compare_and_correlate_state_require_reference_fields() {
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

    let correlate = CorrelateStateArgs {
        reference_time: Some("2026-06-13T00:00:00Z".to_string()),
        window_minutes: Some(10),
        host: Some("host-a".to_string()),
        severity_min: Some("warning".to_string()),
        limit: Some(25),
        json: false,
    }
    .into_request()
    .unwrap();
    assert_eq!(correlate.reference_time, "2026-06-13T00:00:00Z");
    assert_eq!(correlate.host.as_deref(), Some("host-a"));
    assert_eq!(correlate.limit, Some(25));

    assert!(CorrelateStateArgs::default().into_request().is_err());
}

#[test]
fn heartbeat_state_args_map_to_requests() {
    let host = HostStateArgs {
        host_id: Some("host-id".to_string()),
        host: Some("host-a".to_string()),
        since: Some("2026-06-13T00:00:00Z".to_string()),
        limit: Some(20),
        json: true,
    }
    .into_request();
    assert_eq!(host.host_id.as_deref(), Some("host-id"));
    assert_eq!(host.host.as_deref(), Some("host-a"));
    assert_eq!(host.since.as_deref(), Some("2026-06-13T00:00:00Z"));
    assert_eq!(host.limit, Some(20));

    let fleet = FleetStateArgs {
        include_ok: Some(true),
        sort: Some("pressure".to_string()),
        json: false,
    }
    .into_request();
    assert_eq!(fleet.include_ok, Some(true));
    assert_eq!(fleet.sort.as_deref(), Some("pressure"));
}

#[test]
fn graph_args_attach_mode_and_preserve_limits() {
    let entity = EntityArgs {
        entity_type: Some("app".to_string()),
        key: Some("sshd".to_string()),
        alias_type: Some("app_name".to_string()),
        alias_key: Some("ssh".to_string()),
        limit: Some(5),
        evidence_sample_limit: Some(2),
        payload_budget: Some(4096),
        json: false,
    }
    .into_request();
    assert_eq!(entity.mode.as_deref(), Some("entity"));
    assert_eq!(entity.entity_type.as_deref(), Some("app"));
    assert_eq!(entity.payload_budget, Some(4096));

    let around = GraphAroundArgs {
        entity_id: Some(7),
        entity_type: Some("host".to_string()),
        key: Some("host-a".to_string()),
        alias_type: None,
        alias_key: None,
        depth: Some(1),
        limit: Some(20),
        evidence_sample_limit: Some(3),
        payload_budget: Some(8192),
        json: true,
    }
    .into_request();
    assert_eq!(around.mode.as_deref(), Some("around"));
    assert_eq!(around.entity_id, Some(7));
    assert_eq!(around.depth, Some(1));

    let explain = GraphExplainArgs {
        entity_id: Some(7),
        entity_type: None,
        key: None,
        alias_type: None,
        alias_key: None,
        depth: Some(1),
        beam_width: Some(3),
        max_chains: Some(4),
        evidence_sample_limit: Some(2),
        payload_budget: Some(16_384),
        json: false,
    }
    .into_request();
    assert_eq!(explain.mode.as_deref(), Some("explain"));
    assert_eq!(explain.beam_width, Some(3));
    assert_eq!(explain.max_chains, Some(4));

    let evidence = GraphEvidenceArgs {
        evidence_id: 9,
        payload_budget: Some(4096),
        json: false,
    }
    .into_request();
    assert_eq!(evidence.mode.as_deref(), Some("evidence"));
    assert_eq!(evidence.evidence_id, 9);
    assert_eq!(evidence.payload_budget, Some(4096));
}
