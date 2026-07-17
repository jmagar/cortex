use super::*;

#[tokio::test]
async fn missing_optional_config_is_not_a_collection_error() {
    let out = collect(&[], Duration::from_millis(10)).await;
    assert!(out.errors.is_empty());
    assert!(
        out.warnings
            .iter()
            .any(|warning| warning.contains("skipped"))
    );
}

#[test]
fn normalizes_media_version() {
    let svc = MediaServiceConfig {
        kind: "sonarr".to_string(),
        base_url: "http://sonarr".to_string(),
        api_key: None,
        username: None,
        password: None,
    };
    let mut out = CollectorOutput::new("media_stack");
    normalize_service(
        &svc,
        &serde_json::json!({"version":"4.0.0"}),
        "ok",
        &mut out,
    );
    assert_eq!(out.media_services[0].version.as_deref(), Some("4.0.0"));
}

#[test]
fn normalizes_string_media_version() {
    assert_eq!(
        version_from_body(&serde_json::json!("v4.6.5")).as_deref(),
        Some("v4.6.5")
    );
}

#[test]
fn tautulli_request_endpoint_adds_api_key_without_changing_provenance_endpoint() {
    let svc = MediaServiceConfig {
        kind: "tautulli".to_string(),
        base_url: "http://tautulli".to_string(),
        api_key: Some("fixture-value".to_string()),
        username: None,
        password: None,
    };
    let endpoint = endpoint_for(&svc);
    let request_endpoint = request_endpoint_for(&svc, &endpoint);

    assert!(!endpoint.contains("fixture-value"));
    let expected_query = format!("{}=fixture-value", ["api", "key"].concat());
    assert!(request_endpoint.contains(&expected_query));
}
