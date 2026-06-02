#[test]
fn db_vacuum_force_false_omits_force_field() {
    let req = crate::cli::DbVacuumArgs {
        full: true,
        pages: 50,
        force: false,
        json: false,
    }
    .into_request();

    assert!(req.full);
    assert_eq!(req.incremental_pages, 50);
    assert_eq!(req.force, None);
}

// ─── Integrity HTTP timeout (bead cortex-qekb) ──────────────────────────

/// When the server is slower than `http_timeout`, `run_db_integrity_with_timeout`
/// must bail with an actionable message that includes the docker exec command.
#[tokio::test]
async fn run_db_integrity_http_timeout_emits_actionable_message() {
    use crate::cli::http_client::HttpClient;
    use crate::cli::{CliMode, DbIntegrityArgs};
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    // Mock responds after 200ms, well past our injected 10ms timeout.
    Mock::given(method("GET"))
        .and(path("/api/db/integrity"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"ok": true, "messages": []}))
                .set_delay(Duration::from_millis(200)),
        )
        .mount(&server)
        .await;

    let client =
        HttpClient::discover(Some(server.uri()), Some("test-token".into())).expect("discover ok");
    let mode = CliMode::Http(client);

    let err = super::run_db_integrity_with_timeout(
        &mode,
        DbIntegrityArgs {
            quick: true,
            json: false,
            background: false,
        },
        Duration::from_millis(10),
    )
    .await
    .expect_err("must bail on HTTP timeout");

    let msg = err.to_string();
    assert!(
        msg.contains("timed out"),
        "expected 'timed out' in error, got: {msg}"
    );
    assert!(
        msg.contains("docker exec"),
        "expected 'docker exec' guidance in error, got: {msg}"
    );
}
