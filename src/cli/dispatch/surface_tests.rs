#[test]
fn ingest_rate_by_host_maps_to_optional_true() {
    let req = crate::cli::IngestRateArgs {
        by_host: true,
        json: false,
    }
    .into_request();

    assert_eq!(req.by_host, Some(true));
}

#[test]
fn source_ips_timeline_and_patterns_args_map_to_requests() {
    let source_ips = crate::cli::SourceIpsArgs {
        limit: Some(25),
        offset: Some(5),
        json: false,
    }
    .into_request();
    assert_eq!(source_ips.limit, Some(25));
    assert_eq!(source_ips.offset, Some(5));

    let timeline = crate::cli::TimelineArgs {
        bucket: Some("hour".to_string()),
        group_by: Some("app_name".to_string()),
        since: Some("2026-06-13T00:00:00Z".to_string()),
        until: Some("2026-06-13T01:00:00Z".to_string()),
        host: Some("host-a".to_string()),
        app: Some("nginx".to_string()),
        severity_min: Some("warning".to_string()),
        json: true,
    }
    .into_request();
    assert_eq!(timeline.bucket.as_deref(), Some("hour"));
    assert_eq!(timeline.group_by.as_deref(), Some("app_name"));
    assert_eq!(timeline.host.as_deref(), Some("host-a"));
    assert_eq!(timeline.severity_min.as_deref(), Some("warning"));

    let patterns = crate::cli::PatternsArgs {
        since: Some("from".to_string()),
        until: Some("to".to_string()),
        host: Some("host-a".to_string()),
        app: Some("cortex".to_string()),
        severity_min: Some("err".to_string()),
        scan_limit: Some(1000),
        top_n: Some(10),
        json: false,
    }
    .into_request();
    assert_eq!(patterns.app.as_deref(), Some("cortex"));
    assert_eq!(patterns.scan_limit, Some(1000));
    assert_eq!(patterns.top_n, Some(10));
}

#[test]
fn signature_args_map_to_admin_requests() {
    let list = crate::cli::SigListArgs {
        limit: Some(20),
        include_acknowledged: true,
        json: false,
    }
    .into_request();
    assert_eq!(list.limit, Some(20));
    assert_eq!(list.include_acknowledged, Some(true));

    let ack = crate::cli::SigAckArgs {
        signature_hash: "abc123".to_string(),
        notes: Some("handled".to_string()),
        json: true,
    }
    .into_request();
    assert_eq!(ack.signature_hash, "abc123");
    assert_eq!(ack.notes.as_deref(), Some("handled"));

    let unack = crate::cli::SigUnackArgs {
        signature_hash: "abc123".to_string(),
        reason: Some("regressed".to_string()),
        json: false,
    }
    .into_request();
    assert_eq!(unack.signature_hash, "abc123");
    assert_eq!(unack.reason.as_deref(), Some("regressed"));
}

#[test]
fn ingest_rate_by_host_false_maps_to_absent_filter() {
    let req = crate::cli::IngestRateArgs {
        by_host: false,
        json: false,
    }
    .into_request();

    assert_eq!(req.by_host, None);
}
