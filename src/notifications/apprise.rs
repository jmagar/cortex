//! Apprise HTTP client for sending push notifications.
//!
//! Security: NEVER log URLs, X-Apprise-Token, or Authorization headers at any
//! RUST_LOG level. Log rule_id, hostname, severity only.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Type of notification (maps to Apprise notify_type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyType {
    Info,
    Success,
    Warning,
    Failure,
}

impl NotifyType {
    fn as_str(self) -> &'static str {
        match self {
            NotifyType::Info => "info",
            NotifyType::Success => "success",
            NotifyType::Warning => "warning",
            NotifyType::Failure => "failure",
        }
    }
}

/// Response from Apprise notify endpoint.
#[derive(Debug, Clone)]
pub struct NotifyResponse {
    pub status_code: u16,
    /// True when all urls succeeded (200/207); 424 is treated as permanent failure.
    #[allow(dead_code)]
    pub success: bool,
}

/// Errors that can occur when notifying via Apprise.
#[derive(Debug)]
pub enum AppriseError {
    /// Transient error — safe to retry (5xx, timeout, connection refused).
    Transient(String),
    /// Permanent error — do NOT retry (4xx except 429).
    Permanent { code: u16, body: String },
    /// The request timed out.
    Timeout,
}

impl std::fmt::Display for AppriseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppriseError::Transient(msg) => write!(f, "transient: {msg}"),
            AppriseError::Permanent { code, body } => write!(f, "permanent {code}: {body}"),
            AppriseError::Timeout => write!(f, "timeout"),
        }
    }
}

impl std::error::Error for AppriseError {}

/// HTTP client wrapper for the Apprise stateless notify endpoint.
#[derive(Clone)]
pub struct AppriseClient {
    client: reqwest::Client,
    /// Base URL of apprise-api, e.g. "http://apprise:8000".
    base_url: String,
    /// Request timeout.
    timeout: Duration,
}

impl AppriseClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(false)
            .build()
            .expect("reqwest client should build");
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            timeout: Duration::from_secs(5),
        }
    }

    #[allow(dead_code)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// POST a notification to `{base_url}/notify/`.
    ///
    /// Apprise stateless mode: URLs are sent in the request body, not stored
    /// server-side.
    pub async fn notify(
        &self,
        urls: &[String],
        title: &str,
        body: &str,
        notify_type: NotifyType,
    ) -> Result<NotifyResponse, AppriseError> {
        let safe_title = escape_for_notification(title);
        let safe_body = escape_for_notification(body);

        let endpoint = format!("{}/notify/", self.base_url);
        let payload = serde_json::json!({
            "urls": urls,
            "title": safe_title,
            "body": safe_body,
            "type": notify_type.as_str(),
            "format": "markdown",
        });

        let request = self
            .client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            // Note: do NOT log this header value
            .json(&payload)
            .timeout(self.timeout);

        let resp = request.send().await.map_err(|e| {
            if e.is_timeout() {
                AppriseError::Timeout
            } else {
                AppriseError::Transient(format!("send error: {e}"))
            }
        })?;

        let status = resp.status().as_u16();

        match status {
            // 200/201/202/204 = success
            // 207 = partial success — mark sent, do NOT retry
            200 | 201 | 202 | 204 | 207 => Ok(NotifyResponse {
                status_code: status,
                success: true,
            }),
            429 | 500..=599 => {
                // Transient — safe to retry
                Err(AppriseError::Transient(format!("HTTP {status}")))
            }
            300..=399 => Err(AppriseError::Transient(format!("redirect HTTP {status}"))),
            other => {
                // 4xx (excluding 429) = permanent
                let body_text = resp
                    .text()
                    .await
                    .unwrap_or_else(|_| "<body read error>".to_string());
                // Truncate body to avoid logging sensitive content
                let truncated = body_text.chars().take(200).collect::<String>();
                Err(AppriseError::Permanent {
                    code: other,
                    body: truncated,
                })
            }
        }
    }
}

/// Escape log-derived text for safe notification delivery.
///
/// - Replaces `@` with `＠` (U+FF20) to prevent mention injection.
/// - Strips `<` and `>` to prevent HTML/markup injection.
pub fn escape_for_notification(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '@' => Some('＠'),
            '<' | '>' => None,
            other => Some(other),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_replaces_at_sign() {
        assert_eq!(
            escape_for_notification("user@example.com"),
            "user＠example.com"
        );
    }

    #[test]
    fn escape_strips_angle_brackets() {
        assert_eq!(
            escape_for_notification("<script>alert(1)</script>"),
            "scriptalert(1)/script"
        );
    }

    #[test]
    fn escape_combined() {
        assert_eq!(
            escape_for_notification("Out of memory: Killed process 1234 (nginx) <@root>"),
            "Out of memory: Killed process 1234 (nginx) ＠root"
        );
    }

    #[test]
    fn escape_clean_string() {
        let clean = "normal log message without special chars";
        assert_eq!(escape_for_notification(clean), clean);
    }

    /// Test AppriseClient against a mock axum server.
    #[tokio::test]
    async fn mock_server_200() {
        let (client, _server) = make_mock_server(axum::http::StatusCode::OK).await;
        let result = client
            .notify(&["test://".to_string()], "Test", "Body", NotifyType::Info)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().success);
    }

    #[tokio::test]
    async fn mock_server_207_partial_success() {
        let (client, _server) = make_mock_server(axum::http::StatusCode::MULTI_STATUS).await;
        let result = client
            .notify(
                &["test://".to_string()],
                "Test",
                "Body",
                NotifyType::Warning,
            )
            .await;
        assert!(result.is_ok(), "207 should be treated as success");
        assert!(result.unwrap().success);
    }

    #[tokio::test]
    async fn mock_server_400_permanent() {
        let (client, _server) = make_mock_server(axum::http::StatusCode::BAD_REQUEST).await;
        let result = client
            .notify(&["test://".to_string()], "Test", "Body", NotifyType::Info)
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppriseError::Permanent { code, .. } => assert_eq!(code, 400),
            other => panic!("expected Permanent, got {other}"),
        }
    }

    #[tokio::test]
    async fn mock_server_500_transient() {
        let (client, _server) =
            make_mock_server(axum::http::StatusCode::INTERNAL_SERVER_ERROR).await;
        let result = client
            .notify(&["test://".to_string()], "Test", "Body", NotifyType::Info)
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppriseError::Transient(_) => {}
            other => panic!("expected Transient, got {other}"),
        }
    }

    #[tokio::test]
    async fn mock_server_timeout() {
        use tokio::net::TcpListener;

        // Bind but never accept — causes timeout
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");

        // Keep the listener alive so the port exists but no response comes
        let _listener = listener;

        let client = AppriseClient::new(base_url).with_timeout(Duration::from_millis(50));
        let result = client
            .notify(&["test://".to_string()], "Test", "Body", NotifyType::Info)
            .await;
        assert!(result.is_err());
        // Could be Timeout or Transient depending on OS behavior
        match result.unwrap_err() {
            AppriseError::Timeout | AppriseError::Transient(_) => {}
            other => panic!("expected Timeout or Transient, got {other}"),
        }
    }

    // Helper: spin up an axum server that always responds with `status_code`.
    async fn make_mock_server(
        status_code: axum::http::StatusCode,
    ) -> (AppriseClient, tokio::task::JoinHandle<()>) {
        use axum::{routing::post, Router};
        use tokio::net::TcpListener;

        let app = Router::new().route(
            "/notify/",
            post(move || async move { axum::http::StatusCode::from(status_code) }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        let client =
            AppriseClient::new(format!("http://{addr}")).with_timeout(Duration::from_secs(2));
        (client, handle)
    }
}
