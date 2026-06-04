use super::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn get_json_redacts_sensitive_json_values() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "headers": {"x-api-key": "remove-me"}
        })))
        .mount(&server)
        .await;

    let probe = HttpProbe::new(Duration::from_secs(1)).unwrap();
    let result = probe
        .get_json(&format!("{}/data", server.uri()), HeaderMap::new())
        .await
        .unwrap();

    let text = serde_json::to_string(&result.body).unwrap();
    assert!(!text.contains("remove-me"));
    assert!(text.contains("[REDACTED]"));
}

#[tokio::test]
async fn get_json_marks_large_body_truncated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/large"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("x".repeat(MAX_HTTP_BODY_BYTES + 16)),
        )
        .mount(&server)
        .await;

    let probe = HttpProbe::new(Duration::from_secs(1)).unwrap();
    let result = probe
        .get_json(&format!("{}/large", server.uri()), HeaderMap::new())
        .await
        .unwrap();

    assert!(result.truncated);
}

#[test]
fn api_key_header_rejects_invalid_header_values() {
    assert!(api_key_header("x-api-key", "bad\nvalue").is_err());
}
