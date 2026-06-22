use axum::body::to_bytes;
use axum::http::{Request, StatusCode, header};
use tower::util::ServiceExt;

use super::*;

async fn get(path: &str) -> axum::response::Response {
    router()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(path)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn app_route_serves_workspace_shell_with_no_store_and_csp() {
    let response = get("/app/investigate").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    assert!(
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("script-src 'self'")
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Cortex investigation workspace"));
    assert!(html.contains("/app/assets/cytoscape-3.34.0.min.js"));
    assert!(!html.contains("CORTEX_API_TOKEN="));
}

#[tokio::test]
async fn app_spa_fallback_is_scoped_under_app_only() {
    let response = get("/app/some/deep/link").await;
    assert_eq!(response.status(), StatusCode::OK);

    for path in ["/api/stats", "/mcp", "/health", "/v1/logs"] {
        let response = get(path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn app_assets_have_expected_cache_policy() {
    let response = get("/app/assets/app.js").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );

    let response = get("/app/assets/cytoscape-3.34.0.min.js").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "public, max-age=31536000, immutable"
    );
}

#[tokio::test]
async fn app_script_uses_text_nodes_for_dynamic_content() {
    assert!(!APP_JS.contains("innerHTML"));
    assert!(APP_JS.contains("textContent"));
    assert!(APP_JS.contains("document.createElement"));
}

#[tokio::test]
async fn graph_dependency_is_pinned_and_documented() {
    assert!(CYTOSCAPE_JS.contains("cytoscape"));
    assert!(include_str!("../web/vendor/THIRD_PARTY.md").contains("Version: `3.34.0`"));
    assert!(
        include_str!("../web/vendor/cytoscape-3.34.0.package.json")
            .contains("\"name\": \"cytoscape\"")
    );
    assert!(
        include_str!("../web/vendor/cytoscape-3.34.0.package.json")
            .contains("\"license\": \"MIT\"")
    );
    assert!(
        include_str!("../web/vendor/cytoscape-3.34.0.LICENSE")
            .contains("Permission is hereby granted")
    );
}
