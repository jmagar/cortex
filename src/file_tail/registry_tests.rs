use super::models::{FileTailAddRequest, FileTailSource};
use super::registry::FileTailRegistry;

#[test]
fn registry_adds_lists_and_removes_sources() {
    let temp = tempfile::tempdir().unwrap();
    let registry = FileTailRegistry::new(temp.path().join("file-tails.json"));
    let source = FileTailSource::from_add(
        FileTailAddRequest {
            id: "swag-access".into(),
            path: "/tmp/access.log".into(),
            tag: "swag-access".into(),
            hostname: Some("squirts".into()),
            facility: None,
            severity: None,
            start_at_end: None,
        },
        "2026-06-11T20:00:00Z",
    )
    .unwrap();

    registry.upsert(source.clone()).unwrap();
    assert_eq!(registry.list().unwrap(), vec![source]);

    registry.remove("swag-access").unwrap();
    assert!(registry.list().unwrap().is_empty());
}

#[test]
fn registry_persists_across_instances() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("file-tails.json");
    let registry = FileTailRegistry::new(path.clone());
    registry
        .upsert(
            FileTailSource::from_add(
                FileTailAddRequest {
                    id: "authelia".into(),
                    path: "/tmp/authelia.log".into(),
                    tag: "authelia".into(),
                    hostname: None,
                    facility: Some("local5".into()),
                    severity: Some("info".into()),
                    start_at_end: Some(false),
                },
                "2026-06-11T20:00:00Z",
            )
            .unwrap(),
        )
        .unwrap();

    let reloaded = FileTailRegistry::new(path);
    let sources = reloaded.list().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, "authelia");
    assert_eq!(sources[0].facility.as_deref(), Some("local5"));
    assert!(!sources[0].start_at_end);
}

#[test]
fn registry_remove_missing_source_returns_error() {
    let temp = tempfile::tempdir().unwrap();
    let registry = FileTailRegistry::new(temp.path().join("file-tails.json"));

    let err = registry.remove("missing").unwrap_err();

    assert!(err.to_string().contains("not found"));
}
