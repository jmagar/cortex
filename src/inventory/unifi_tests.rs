use super::*;

#[tokio::test]
async fn missing_optional_config_is_not_a_collection_error() {
    let out = collect(None, None, Duration::from_millis(10)).await;
    assert!(out.errors.is_empty());
    assert!(
        out.warnings
            .iter()
            .any(|warning| warning.contains("skipped"))
    );
}
use serde_json::json;

#[test]
fn unifi_optional_fields_do_not_break_device_normalization() {
    let mut out = CollectorOutput::new("unifi");
    normalize_unifi(
        "https://unifi",
        "/proxy/network/api/s/default/stat/device",
        &json!({"data":[{"mac":"aa:bb","ip":"10.0.0.2"}]}),
        &mut out,
    );
    assert_eq!(out.nodes[0].hostname, "aa:bb");
    assert_eq!(out.nodes[0].ips, vec!["10.0.0.2"]);
}

#[test]
fn unifi_truncation_and_missing_device_ids_are_reported() {
    let mut out = CollectorOutput::new("unifi");
    let items = (0..201)
        .map(|idx| json!({"id": format!("dev-{idx}")}))
        .collect::<Vec<_>>();
    normalize_unifi(
        "https://unifi",
        "/proxy/network/api/s/default/stat/device",
        &json!({"data": items}),
        &mut out,
    );

    assert_eq!(out.nodes.len(), 200);
    assert!(out.errors.iter().any(|error| error.truncated));

    let mut out = CollectorOutput::new("unifi");
    normalize_unifi(
        "https://unifi",
        "/proxy/network/api/s/default/stat/device",
        &json!({"data":[{"ip":"10.0.0.2"}]}),
        &mut out,
    );
    assert!(out.nodes.is_empty());
    assert!(
        out.errors
            .iter()
            .any(|error| error.message.contains("missing"))
    );
}

#[tokio::test]
async fn invalid_api_key_header_reports_config_warning() {
    let out = collect(
        Some("https://unifi"),
        Some("bad\nkey"),
        std::time::Duration::from_millis(10),
    )
    .await;
    assert!(out.errors.iter().any(|error| {
        error.phase == "config" && error.message.contains("invalid header characters")
    }));
}
