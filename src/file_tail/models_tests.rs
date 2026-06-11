use super::models::*;

#[test]
fn add_request_builds_enabled_source_with_defaults() {
    let req = FileTailAddRequest {
        id: "swag-access".into(),
        path: "/mnt/appdata/swag/log/nginx/access.log".into(),
        tag: "swag-access".into(),
        hostname: Some("squirts".into()),
        facility: None,
        severity: None,
        start_at_end: None,
    };

    let source = FileTailSource::from_add(req, "2026-06-11T20:00:00Z");

    assert_eq!(source.id, "swag-access");
    assert_eq!(source.path, "/mnt/appdata/swag/log/nginx/access.log");
    assert_eq!(source.tag, "swag-access");
    assert_eq!(source.hostname.as_deref(), Some("squirts"));
    assert_eq!(source.facility.as_deref(), Some("local7"));
    assert_eq!(source.severity, "info");
    assert!(source.start_at_end);
    assert!(source.enabled);
    assert_eq!(source.created_at, "2026-06-11T20:00:00Z");
    assert_eq!(source.updated_at, "2026-06-11T20:00:00Z");
}

#[test]
fn file_tail_request_rejects_missing_fields_for_add() {
    let req = FileTailRequest {
        op: FileTailOp::Add,
        id: None,
        path: None,
        tag: None,
        hostname: None,
        facility: None,
        severity: None,
        start_at_end: None,
    };

    assert_eq!(
        req.validate().unwrap_err(),
        "file_tails op=add requires id, path, and tag"
    );
}

#[test]
fn file_tail_request_rejects_path_traversal_ids() {
    let req = FileTailRequest {
        op: FileTailOp::Remove,
        id: Some("../swag".into()),
        path: None,
        tag: None,
        hostname: None,
        facility: None,
        severity: None,
        start_at_end: None,
    };

    assert_eq!(
        req.validate().unwrap_err(),
        "file_tails id must contain only ASCII letters, digits, dot, underscore, or dash"
    );
}
