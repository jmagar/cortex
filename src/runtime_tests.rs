use std::path::PathBuf;

use super::{background_interval, build_auth_policy};
use crate::config::{AuthConfig, AuthMode, Config, McpConfig, StorageConfig};
use crate::mcp::AuthPolicy;

#[tokio::test]
async fn background_interval_waits_full_period_before_first_tick() {
    let delay = tokio::time::Duration::from_millis(25);
    let mut interval = background_interval(delay);
    let started = std::time::Instant::now();
    interval.tick().await;
    assert!(
        started.elapsed() >= tokio::time::Duration::from_millis(20),
        "first tick should wait roughly one full period before firing"
    );
}

/// Build a minimal `Config` rooted at `tmp` with the supplied overrides.
fn test_config(tmp: &std::path::Path, mcp: McpConfig) -> Config {
    let storage = StorageConfig::for_test(tmp.join("syslog.db"));
    Config {
        syslog: Default::default(),
        storage,
        mcp,
        api: Default::default(),
        docker_ingest: Default::default(),
        enrichment: Default::default(),
    }
}

fn loopback_mcp() -> McpConfig {
    McpConfig {
        host: "127.0.0.1".into(),
        port: 3100,
        server_name: "syslog-mcp".into(),
        api_token: None,
        allowed_hosts: Vec::new(),
        allowed_origins: Vec::new(),
        auth: AuthConfig::default(),
    }
}

fn oauth_mcp(tmp: &std::path::Path) -> McpConfig {
    let mut mcp = loopback_mcp();
    mcp.auth = AuthConfig {
        mode: AuthMode::OAuth,
        public_url: Some("https://syslog.example.com".into()),
        google_client_id: Some("client-id".into()),
        google_client_secret: Some("client-secret".into()),
        admin_email: "admin@example.com".into(),
        allowed_emails: Vec::new(),
        sqlite_path: tmp.join("auth.db"),
        key_path: tmp.join("auth-jwt.pem"),
        access_token_ttl_secs: 3_600,
        refresh_token_ttl_secs: 28_800,
        auth_code_ttl_secs: 300,
        register_rpm: 20,
        authorize_rpm: 60,
        disable_static_token_with_oauth: true,
        allowed_client_redirect_uris: Vec::new(),
    };
    mcp
}

#[tokio::test]
async fn build_auth_policy_returns_loopback_dev_when_no_auth_and_loopback_bind() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path(), loopback_mcp());
    let policy = build_auth_policy(&config, false)
        .await
        .expect("build policy");
    assert!(matches!(policy, AuthPolicy::LoopbackDev));
}

#[tokio::test]
async fn build_auth_policy_returns_mounted_bearer_only_when_static_token_only() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mcp = loopback_mcp();
    mcp.api_token = Some("supersecret".into());
    mcp.host = "0.0.0.0".into();
    let config = test_config(tmp.path(), mcp);
    // Bearer-only: AuthLayer is mounted (auth is enforced), but no OAuth state.
    // Scope checks in S5 must still run — Mounted { auth_state: None } is correct.
    let policy = build_auth_policy(&config, false)
        .await
        .expect("build policy");
    assert!(matches!(policy, AuthPolicy::Mounted { auth_state: None }));
}

#[tokio::test]
async fn build_auth_policy_returns_mounted_when_oauth_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path(), oauth_mcp(tmp.path()));
    let policy = build_auth_policy(&config, false)
        .await
        .expect("oauth init should succeed");
    assert!(matches!(
        policy,
        AuthPolicy::Mounted {
            auth_state: Some(_)
        }
    ));

    // The lab-auth files must exist after init.
    assert!(tmp.path().join("auth.db").exists(), "auth.db missing");
    assert!(
        tmp.path().join("auth-jwt.pem").exists(),
        "auth-jwt.pem missing"
    );
}

#[tokio::test]
async fn build_auth_policy_propagates_lab_auth_errors() {
    // OAuth mode with an invalid public_url (not a URL) → AuthState::new fails.
    let tmp = tempfile::tempdir().unwrap();
    let mut mcp = oauth_mcp(tmp.path());
    mcp.auth.public_url = Some("not a url".into());
    let config = test_config(tmp.path(), mcp);
    let err = build_auth_policy(&config, false)
        .await
        .expect_err("invalid public_url should fail");
    let msg = format!("{err:#}");
    assert!(
        msg.to_ascii_lowercase().contains("public_url") || msg.to_ascii_lowercase().contains("url"),
        "error should mention url; got: {msg}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn build_auth_policy_enforces_restrictive_permissions_on_auth_db() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path(), oauth_mcp(tmp.path()));
    let _policy = build_auth_policy(&config, false).await.expect("oauth init");

    let db_path: PathBuf = tmp.path().join("auth.db");
    let mode = std::fs::metadata(&db_path)
        .expect("stat auth.db")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode & 0o077,
        0,
        "auth.db must be 0600 (group/other bits clear); got mode={:o}",
        mode
    );

    let key_path: PathBuf = tmp.path().join("auth-jwt.pem");
    let key_mode = std::fs::metadata(&key_path)
        .expect("stat auth-jwt.pem")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        key_mode & 0o077,
        0,
        "auth-jwt.pem must be 0600 (group/other bits clear); got mode={:o}",
        key_mode
    );
}
