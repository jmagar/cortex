use super::*;
use serial_test::serial;

#[test]
#[serial]
fn cortex_token_sets_api_token() {
    std::env::set_var("CORTEX_TOKEN", "test-token");
    std::env::remove_var("CORTEX_API_TOKEN");
    let result = Config::load();
    std::env::remove_var("CORTEX_TOKEN");

    let cfg = result.expect("Config::load() should succeed");
    assert_eq!(cfg.mcp.api_token, Some("test-token".into()));
}

#[test]
#[serial]
fn api_token_env_sets_api_token_not_mcp_token() {
    // cortex v1.0.0: the pre-v1 `SYSLOG_MCP_API_TOKEN` deprecated MCP-token alias was
    // dropped. Its post-rename name `CORTEX_API_TOKEN` now belongs exclusively to the
    // API/OTLP token (`config.api.api_token`) and must NOT set the MCP static token.
    std::env::remove_var("CORTEX_TOKEN");
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_API_TOKEN", "api-token");
    let result = Config::load();
    std::env::remove_var("CORTEX_API_TOKEN");
    std::env::remove_var("CORTEX_HOST");

    let cfg = result.expect("Config::load() should succeed");
    assert_eq!(cfg.api.api_token, Some("api-token".into()));
    assert_eq!(cfg.mcp.api_token, None);
}

#[test]
#[serial]
fn env_var_overrides_mcp_port() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_PORT", "3200");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_PORT");

    let cfg = result.expect("Config::load() should succeed");
    assert_eq!(cfg.mcp.port, 3200);
}

#[test]
#[serial]
fn env_var_overrides_receiver_port() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_RECEIVER_PORT", "2514");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_RECEIVER_PORT");

    let cfg = result.expect("Config::load() should succeed");
    assert_eq!(cfg.receiver.port, 2514);
    assert_eq!(cfg.receiver.bind_addr(), "0.0.0.0:2514");
}

#[test]
fn receiver_toml_section_deserializes_into_receiver_field() {
    // Guards the field/section coupling: the `[receiver]` TOML section must bind to
    // `Config.receiver`. A serde mismatch here compiles clean and only fails at runtime,
    // so this positive round-trip is the explicit guard (no env vars, pure deserialize).
    let raw = "[receiver]\nhost = \"10.0.0.5\"\nport = 2514\nbatch_size = 250\n";
    let cfg: Config = toml::from_str(raw).expect("[receiver] section should parse");
    assert_eq!(cfg.receiver.host, "10.0.0.5");
    assert_eq!(cfg.receiver.port, 2514);
    assert_eq!(cfg.receiver.batch_size, 250);
}

#[test]
#[serial]
fn defaults_are_applied_without_env_vars() {
    // Clear any leaked env vars
    for key in [
        "CORTEX_RECEIVER_HOST",
        "CORTEX_RECEIVER_PORT",
        "CORTEX_MAX_MESSAGE_SIZE",
        "CORTEX_MAX_TCP_CONNECTIONS",
        "CORTEX_TCP_IDLE_TIMEOUT_SECS",
        "CORTEX_BATCH_SIZE",
        "CORTEX_FLUSH_INTERVAL",
        "CORTEX_HOST",
        "CORTEX_PORT",
        "CORTEX_ALLOWED_HOSTS",
        "CORTEX_ALLOWED_ORIGINS",
        "NO_AUTH",
        "CORTEX_NO_AUTH",
        "CORTEX_DB_PATH",
        "CORTEX_POOL_SIZE",
        "CORTEX_RETENTION_DAYS",
        "CORTEX_TOKEN",
        "CORTEX_API_TOKEN",
        "CORTEX_MAX_DB_SIZE_MB",
        "CORTEX_RECOVERY_DB_SIZE_MB",
        "CORTEX_MIN_FREE_DISK_MB",
        "CORTEX_RECOVERY_FREE_DISK_MB",
        "CORTEX_CLEANUP_INTERVAL_SECS",
        "CORTEX_CLEANUP_CHUNK_SIZE",
        "CORTEX_API_TOKEN",
        "CORTEX_WRITE_CHANNEL_CAPACITY",
        "CORTEX_DOCKER_INGEST_ENABLED",
        "CORTEX_DOCKER_HOSTS_FILE",
        "CORTEX_DOCKER_RECONNECT_INITIAL_MS",
        "CORTEX_DOCKER_RECONNECT_MAX_MS",
        "CORTEX_AUTH_MODE",
        "CORTEX_PUBLIC_URL",
        "CORTEX_GOOGLE_CLIENT_ID",
        "CORTEX_GOOGLE_CLIENT_SECRET",
        "CORTEX_AUTH_ADMIN_EMAIL",
        "CORTEX_AUTH_ALLOWED_REDIRECT_URIS",
        "CORTEX_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH",
    ] {
        std::env::remove_var(key);
    }

    // Bind cortex to loopback so the non-loopback safety gate (added in
    // cortex-brt0.4) does not reject the unauthenticated default config.
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    let cfg = Config::load().expect("Config::load() should succeed with defaults");
    std::env::remove_var("CORTEX_HOST");
    assert_eq!(cfg.receiver.host, "0.0.0.0");
    assert_eq!(cfg.receiver.port, 1514);
    assert_eq!(cfg.receiver.bind_addr(), "0.0.0.0:1514");
    assert_eq!(cfg.receiver.write_channel_capacity, 10_000);
    assert_eq!(cfg.mcp.host, "127.0.0.1");
    assert_eq!(cfg.mcp.port, 3100);
    assert!(!cfg.mcp.no_auth);
    assert_eq!(cfg.mcp.bind_addr(), "127.0.0.1:3100");
    assert!(cfg.mcp.allowed_hosts.is_empty());
    assert!(cfg.mcp.allowed_origins.is_empty());
    assert_eq!(cfg.storage.pool_size, 4);
    assert_eq!(cfg.storage.retention_days, 90);
    assert!(cfg.storage.wal_mode);
    assert_eq!(cfg.storage.max_db_size_mb, 1024);
    assert_eq!(cfg.storage.recovery_db_size_mb, 900);
    assert_eq!(cfg.storage.min_free_disk_mb, 512);
    assert_eq!(cfg.storage.recovery_free_disk_mb, 768);
    assert_eq!(cfg.storage.cleanup_interval_secs, 60);
    assert_eq!(cfg.storage.cleanup_chunk_size, 2_000);
    assert!(cfg.mcp.api_token.is_none());
    assert!(cfg.api.api_token.is_none());
    assert!(!cfg.docker_ingest.enabled);
    assert!(cfg.docker_ingest.hosts.is_empty());
    assert_eq!(cfg.docker_ingest.reconnect_initial_ms, 1_000);
    assert_eq!(cfg.docker_ingest.reconnect_max_ms, 30_000);
}

#[test]
#[serial]
fn rejects_invalid_syslog_ingest_env_settings() {
    for (key, expected) in [
        ("CORTEX_MAX_MESSAGE_SIZE", "max_message_size"),
        ("CORTEX_MAX_TCP_CONNECTIONS", "max_tcp_connections"),
        ("CORTEX_TCP_IDLE_TIMEOUT_SECS", "tcp_idle_timeout_secs"),
        ("CORTEX_BATCH_SIZE", "batch_size"),
        ("CORTEX_FLUSH_INTERVAL", "flush_interval"),
        ("CORTEX_WRITE_CHANNEL_CAPACITY", "write_channel_capacity"),
    ] {
        std::env::set_var(key, "0");
        let result = Config::load();
        std::env::remove_var(key);

        let err = result.expect_err(&format!("Config::load should reject {key}=0"));
        assert!(
            err.to_string().contains(expected),
            "expected {key}=0 error to mention {expected}, got: {err}"
        );
    }
}

#[test]
fn rejects_invalid_syslog_ingest_toml_settings() {
    for (toml, expected) in [
        ("[receiver]\nmax_message_size = 0\n", "max_message_size"),
        (
            "[receiver]\nmax_tcp_connections = 0\n",
            "max_tcp_connections",
        ),
        (
            "[receiver]\ntcp_idle_timeout_secs = 0\n",
            "tcp_idle_timeout_secs",
        ),
        ("[receiver]\nbatch_size = 0\n", "batch_size"),
        ("[receiver]\nflush_interval = 0\n", "flush_interval"),
        (
            "[receiver]\nwrite_channel_capacity = 0\n",
            "write_channel_capacity",
        ),
    ] {
        let mut config: Config = toml::from_str(toml).unwrap();
        let err = validate_receiver_config(&config.receiver)
            .expect_err(&format!("validate_receiver_config should reject {toml}"));
        assert!(
            err.to_string().contains(expected),
            "expected TOML error to mention {expected}, got: {err}"
        );

        config.receiver = ReceiverConfig::default();
        validate_receiver_config(&config.receiver).unwrap();
    }
}

#[test]
#[serial]
fn env_var_overrides_write_channel_capacity() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_WRITE_CHANNEL_CAPACITY", "100000");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_WRITE_CHANNEL_CAPACITY");

    let cfg = result.expect("Config::load() should parse write channel capacity");
    assert_eq!(cfg.receiver.write_channel_capacity, 100_000);
}

#[test]
#[serial]
fn env_var_overrides_mcp_allowed_hosts_and_origins() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var(
        "CORTEX_ALLOWED_HOSTS",
        "syslog.example.com, syslog.example.com:443",
    );
    std::env::set_var(
        "CORTEX_ALLOWED_ORIGINS",
        "https://app.example.com, https://syslog.example.com",
    );
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_ALLOWED_HOSTS");
    std::env::remove_var("CORTEX_ALLOWED_ORIGINS");

    let cfg = result.expect("Config::load() should parse comma-separated RMCP allow lists");
    assert_eq!(
        cfg.mcp.allowed_hosts,
        vec!["syslog.example.com", "syslog.example.com:443"]
    );
    assert_eq!(
        cfg.mcp.allowed_origins,
        vec!["https://app.example.com", "https://syslog.example.com"]
    );
}

#[test]
#[serial]
fn env_var_can_clear_mcp_allowed_hosts_and_origins() {
    let mut hosts = vec!["syslog.example.com".to_string()];
    let mut origins = vec!["https://syslog.example.com".to_string()];
    std::env::set_var("CORTEX_ALLOWED_HOSTS", "  , ");
    std::env::set_var("CORTEX_ALLOWED_ORIGINS", "");

    env_override_list("CORTEX_ALLOWED_HOSTS", &mut hosts);
    env_override_list("CORTEX_ALLOWED_ORIGINS", &mut origins);

    std::env::remove_var("CORTEX_ALLOWED_HOSTS");
    std::env::remove_var("CORTEX_ALLOWED_ORIGINS");

    assert!(hosts.is_empty());
    assert!(origins.is_empty());
}

#[test]
#[serial]
fn api_token_loads_independently_of_mcp_token() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_API_TOKEN", "api-token");
    std::env::set_var("CORTEX_TOKEN", "mcp-token");
    let result = Config::load();
    std::env::remove_var("CORTEX_API_TOKEN");
    std::env::remove_var("CORTEX_TOKEN");

    let cfg = result.expect("Config::load() should accept distinct API + MCP tokens");
    assert_eq!(cfg.api.api_token, Some("api-token".into()));
    assert_eq!(cfg.mcp.api_token, Some("mcp-token".into()));
}

#[test]
fn auth_validation_rejects_blank_mcp_token() {
    let mut cfg = Config::default();
    cfg.mcp.api_token = Some("  ".into());

    let err = validate_auth_config(&cfg, true).unwrap_err();
    assert!(err.to_string().contains("mcp.api_token"));
}

#[test]
fn auth_validation_rejects_blank_api_token() {
    let mut cfg = Config::default();
    cfg.api.api_token = Some("\t".into());

    let err = validate_auth_config(&cfg, true).unwrap_err();
    assert!(err.to_string().contains("api.api_token"));
}

#[test]
#[serial]
fn host_with_port_is_rejected() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_RECEIVER_HOST", "0.0.0.0:1514");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_RECEIVER_HOST");

    let err = result.expect_err("Host containing ':' should be rejected");
    assert!(
        err.to_string().contains("should not contain a port"),
        "wrong error: {err}"
    );
}

#[test]
fn defaults_include_storage_budget_settings() {
    let cfg = Config::default();
    assert_eq!(cfg.storage.max_db_size_mb, 1024);
    assert_eq!(cfg.storage.recovery_db_size_mb, 900);
    // syslog-mcp-w4hh: free-disk guardrail defaults to 0 (disabled) so cortex
    // does not self-wipe to chase external whole-filesystem pressure. The two
    // free-disk fields MUST default to 0 together to pass validate_storage_config.
    assert_eq!(cfg.storage.min_free_disk_mb, 0);
    assert_eq!(cfg.storage.recovery_free_disk_mb, 0);
    assert_eq!(cfg.storage.cleanup_interval_secs, 60);
}

#[test]
#[serial]
fn env_var_overrides_storage_budget_settings() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_MAX_DB_SIZE_MB", "2048");
    std::env::set_var("CORTEX_RECOVERY_DB_SIZE_MB", "1800");
    std::env::set_var("CORTEX_MIN_FREE_DISK_MB", "1024");
    std::env::set_var("CORTEX_RECOVERY_FREE_DISK_MB", "1536");
    std::env::set_var("CORTEX_CLEANUP_INTERVAL_SECS", "120");

    let result = Config::load();

    for key in [
        "CORTEX_HOST",
        "CORTEX_MAX_DB_SIZE_MB",
        "CORTEX_RECOVERY_DB_SIZE_MB",
        "CORTEX_MIN_FREE_DISK_MB",
        "CORTEX_RECOVERY_FREE_DISK_MB",
        "CORTEX_CLEANUP_INTERVAL_SECS",
    ] {
        std::env::remove_var(key);
    }

    let cfg = result.expect("Config::load() should succeed");
    assert_eq!(cfg.storage.max_db_size_mb, 2048);
    assert_eq!(cfg.storage.recovery_db_size_mb, 1800);
    assert_eq!(cfg.storage.min_free_disk_mb, 1024);
    assert_eq!(cfg.storage.recovery_free_disk_mb, 1536);
    assert_eq!(cfg.storage.cleanup_interval_secs, 120);
}

#[test]
#[serial]
fn rejects_invalid_storage_budget_relationships() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_MAX_DB_SIZE_MB", "100");
    std::env::set_var("CORTEX_RECOVERY_DB_SIZE_MB", "100");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_MAX_DB_SIZE_MB");
    std::env::remove_var("CORTEX_RECOVERY_DB_SIZE_MB");

    let err = result.expect_err("Config::load() should reject invalid recovery_db_size_mb");
    assert!(err.to_string().contains("recovery_db_size_mb"));
}

#[test]
#[serial]
fn rejects_cleanup_chunk_size_zero() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_CLEANUP_CHUNK_SIZE", "0");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_CLEANUP_CHUNK_SIZE");

    let err = result.expect_err("Config::load() should reject cleanup_chunk_size == 0");
    assert!(err.to_string().contains("cleanup_chunk_size"));
}

#[test]
#[serial]
fn rejects_cleanup_chunk_size_over_max() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_CLEANUP_CHUNK_SIZE", "1000001");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_CLEANUP_CHUNK_SIZE");

    let err = result.expect_err("Config::load() should reject cleanup_chunk_size > 1_000_000");
    assert!(
        err.to_string().contains("cleanup_chunk_size"),
        "Expected error referencing cleanup_chunk_size, got: {err}"
    );
}

#[test]
#[serial]
fn accepts_cleanup_chunk_size_at_max() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_CLEANUP_CHUNK_SIZE", "1000000");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_CLEANUP_CHUNK_SIZE");

    let cfg = result.expect("cleanup_chunk_size == 1_000_000 should be accepted");
    assert_eq!(cfg.storage.cleanup_chunk_size, 1_000_000);
}

#[test]
fn docker_ingest_toml_hosts_parse() {
    let raw = r#"
        [docker_ingest]
        enabled = true
        reconnect_initial_ms = 250
        reconnect_max_ms = 10000
        [[docker_ingest.hosts]]
        name = "edge-host-a"
        base_url = "http://edge-host-a:2375"
        allow_insecure_http = true

        [[docker_ingest.hosts]]
        name = "app-host-b"
        base_url = "http://app-host-b:2375"
        allow_insecure_http = true
    "#;

    let config: Config = toml::from_str(raw).unwrap();
    assert!(config.docker_ingest.enabled);
    assert_eq!(config.docker_ingest.hosts.len(), 2);
    assert_eq!(config.docker_ingest.hosts[0].name, "edge-host-a");
    assert_eq!(
        config.docker_ingest.hosts[0].base_url,
        "http://edge-host-a:2375"
    );
    assert_eq!(config.docker_ingest.hosts[1].name, "app-host-b");
    assert_eq!(
        config.docker_ingest.hosts[1].base_url,
        "http://app-host-b:2375"
    );
}

#[test]
fn docker_ingest_requires_hosts_when_enabled() {
    let mut config = DockerIngestConfig {
        enabled: true,
        ..Default::default()
    };
    config.hosts.clear();

    let err = validate_docker_ingest_config(&config).unwrap_err();
    assert!(
        err.to_string()
            .contains("docker_ingest.hosts must not be empty")
    );
}

#[test]
fn docker_ingest_rejects_duplicate_host_names() {
    let config = DockerIngestConfig {
        enabled: true,
        hosts: vec![
            DockerHostConfig {
                name: "edge-host-a".into(),
                base_url: "http://edge-host-a:2375".into(),
                allow_insecure_http: true,
                excluded_containers: Vec::new(),
            },
            DockerHostConfig {
                name: "edge-host-a".into(),
                base_url: "http://10.0.0.10:2375".into(),
                allow_insecure_http: true,
                excluded_containers: Vec::new(),
            },
        ],
        ..Default::default()
    };

    let err = validate_docker_ingest_config(&config).unwrap_err();
    assert!(
        err.to_string()
            .contains("duplicate docker_ingest host name")
    );
}

#[test]
#[serial]
fn docker_ingest_loads_hosts_file_from_env() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("docker-hosts.toml");
    std::fs::write(
        &path,
        r#"
            [[hosts]]
            name = "edge-host-a"
            base_url = "http://edge-host-a:2375"
            allow_insecure_http = true
        "#,
    )
    .unwrap();

    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_DOCKER_INGEST_ENABLED", "true");
    std::env::set_var("CORTEX_DOCKER_HOSTS_FILE", &path);
    // Ensure the host list env var doesn't override the file path under test
    std::env::remove_var("CORTEX_DOCKER_HOSTS");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_DOCKER_INGEST_ENABLED");
    std::env::remove_var("CORTEX_DOCKER_HOSTS_FILE");

    let config = result.expect("Config::load should parse docker host file");
    assert!(config.docker_ingest.enabled);
    assert_eq!(config.docker_ingest.hosts.len(), 1);
    assert_eq!(config.docker_ingest.hosts[0].name, "edge-host-a");
}

#[test]
#[serial]
fn docker_ingest_loads_excluded_containers_from_env() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_DOCKER_INGEST_ENABLED", "true");
    std::env::set_var("CORTEX_DOCKER_HOSTS", "edge-host-a");
    std::env::set_var(
        "CORTEX_DOCKER_EXCLUDED_CONTAINERS",
        "arcane-mcp, axon-qdrant",
    );

    let result = Config::load();

    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_DOCKER_INGEST_ENABLED");
    std::env::remove_var("CORTEX_DOCKER_HOSTS");
    std::env::remove_var("CORTEX_DOCKER_EXCLUDED_CONTAINERS");

    let config = result.expect("Config::load should parse excluded container list");
    assert_eq!(
        config.docker_ingest.excluded_containers,
        vec!["arcane-mcp".to_string(), "axon-qdrant".to_string()]
    );
}

#[test]
#[serial]
fn docker_ingest_ignores_hosts_file_when_disabled() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_DOCKER_INGEST_ENABLED", "false");
    std::env::set_var(
        "CORTEX_DOCKER_HOSTS_FILE",
        "/tmp/cortex-missing-docker-hosts.toml",
    );
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_DOCKER_INGEST_ENABLED");
    std::env::remove_var("CORTEX_DOCKER_HOSTS_FILE");

    let config = result.expect("disabled Docker ingest should ignore stale hosts file env");
    assert!(!config.docker_ingest.enabled);
}

#[test]
fn docker_ingest_rejects_insecure_http_without_explicit_opt_in() {
    let config = DockerIngestConfig {
        enabled: true,
        hosts: vec![DockerHostConfig {
            name: "edge-host-a".into(),
            base_url: "http://edge-host-a:2375".into(),
            allow_insecure_http: false,
            excluded_containers: Vec::new(),
        }],
        ..Default::default()
    };

    let err = validate_docker_ingest_config(&config).unwrap_err();
    assert!(err.to_string().contains("allow_insecure_http"));
}

// ---------------------------------------------------------------------------
// [mcp.auth] config schema (cortex-brt0.4)
// ---------------------------------------------------------------------------

/// Build a baseline loopback-bound config with a static token. Tests start
/// from this and mutate the AuthConfig in isolation.
fn loopback_config_with_token() -> Config {
    let mut cfg = Config::default();
    cfg.mcp.host = "127.0.0.1".into();
    cfg.mcp.api_token = Some("static-token".into());
    cfg
}

fn valid_oauth_config_without_token() -> Config {
    let mut cfg = Config::default();
    cfg.mcp.api_token = None;
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.public_url = Some("https://syslog.example.com".into());
    cfg.mcp.auth.google_client_id = Some("id".into());
    cfg.mcp.auth.google_client_secret = Some("secret".into());
    cfg.mcp.auth.admin_email = "admin@example.com".into();
    cfg
}

#[test]
fn auth_defaults_are_bearer_with_disable_static_token_enabled() {
    let cfg = AuthConfig::default();
    assert_eq!(cfg.mode, AuthMode::Bearer);
    assert!(cfg.public_url.is_none());
    assert!(cfg.google_client_id.is_none());
    assert!(cfg.google_client_secret.is_none());
    assert!(cfg.admin_email.is_empty());
    assert!(cfg.allowed_emails.is_empty());
    assert_eq!(cfg.access_token_ttl_secs, 3_600);
    assert_eq!(cfg.refresh_token_ttl_secs, 28_800);
    assert_eq!(cfg.auth_code_ttl_secs, 300);
    assert_eq!(cfg.register_rpm, 20);
    assert_eq!(cfg.authorize_rpm, 60);
    assert!(
        cfg.disable_static_token_with_oauth,
        "cortex default flips lab-auth's opt-in to opt-out"
    );
    assert!(cfg.allowed_client_redirect_uris.is_empty());
    assert_eq!(cfg.sqlite_path, std::path::PathBuf::from("auth.db"));
    assert_eq!(cfg.key_path, std::path::PathBuf::from("auth-jwt.pem"));
}

#[test]
#[serial]
fn config_load_defaults_to_bearer_mode() {
    std::env::remove_var("CORTEX_AUTH_MODE");
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");

    let cfg = result.expect("loopback bind, no token, no oauth → permitted");
    assert_eq!(cfg.mcp.auth.mode, AuthMode::Bearer);
}

#[test]
#[serial]
fn cortex_auth_mode_env_flips_to_oauth() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_AUTH_MODE", "oauth");
    std::env::set_var("CORTEX_PUBLIC_URL", "https://syslog.example.com");
    std::env::set_var("CORTEX_GOOGLE_CLIENT_ID", "client-id");
    std::env::set_var("CORTEX_GOOGLE_CLIENT_SECRET", "client-secret");
    std::env::set_var("CORTEX_AUTH_ADMIN_EMAIL", "admin@example.com");
    let result = Config::load();
    for k in [
        "CORTEX_HOST",
        "CORTEX_AUTH_MODE",
        "CORTEX_PUBLIC_URL",
        "CORTEX_GOOGLE_CLIENT_ID",
        "CORTEX_GOOGLE_CLIENT_SECRET",
        "CORTEX_AUTH_ADMIN_EMAIL",
    ] {
        std::env::remove_var(k);
    }

    let cfg = result.expect("oauth env overrides should satisfy startup validation");
    assert_eq!(cfg.mcp.auth.mode, AuthMode::OAuth);
    assert_eq!(cfg.mcp.auth.admin_email, "admin@example.com");
}

#[test]
#[serial]
fn cortex_auth_mode_env_rejects_invalid_value() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_AUTH_MODE", "magic");
    let result = Config::load();
    std::env::remove_var("CORTEX_HOST");
    std::env::remove_var("CORTEX_AUTH_MODE");

    let err = result.expect_err("bogus AUTH_MODE must be rejected");
    assert!(err.to_string().contains("CORTEX_AUTH_MODE"));
}

#[test]
#[serial]
fn auth_env_overrides_propagate_to_config() {
    std::env::set_var("CORTEX_HOST", "127.0.0.1");
    std::env::set_var("CORTEX_PUBLIC_URL", "https://syslog.example.com");
    std::env::set_var("CORTEX_GOOGLE_CLIENT_ID", "id-from-env");
    std::env::set_var("CORTEX_GOOGLE_CLIENT_SECRET", "secret-from-env");
    std::env::set_var("CORTEX_AUTH_ADMIN_EMAIL", "admin-from-env@example.com");
    std::env::set_var(
        "CORTEX_AUTH_ALLOWED_REDIRECT_URIS",
        "https://callback.example.com/callback,https://claude.ai/api/mcp/auth_callback",
    );
    std::env::set_var("CORTEX_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH", "false");
    // Stay in bearer mode so validation doesn't require an allowlist.
    std::env::remove_var("CORTEX_AUTH_MODE");
    let result = Config::load();
    for k in [
        "CORTEX_HOST",
        "CORTEX_PUBLIC_URL",
        "CORTEX_GOOGLE_CLIENT_ID",
        "CORTEX_GOOGLE_CLIENT_SECRET",
        "CORTEX_AUTH_ADMIN_EMAIL",
        "CORTEX_AUTH_ALLOWED_REDIRECT_URIS",
        "CORTEX_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH",
    ] {
        std::env::remove_var(k);
    }

    let cfg = result.expect("env overrides should land in config");
    assert_eq!(
        cfg.mcp.auth.public_url.as_deref(),
        Some("https://syslog.example.com")
    );
    assert_eq!(
        cfg.mcp.auth.google_client_id.as_deref(),
        Some("id-from-env")
    );
    assert_eq!(
        cfg.mcp.auth.google_client_secret.as_deref(),
        Some("secret-from-env")
    );
    assert_eq!(cfg.mcp.auth.admin_email, "admin-from-env@example.com");
    assert_eq!(
        cfg.mcp.auth.allowed_client_redirect_uris,
        vec![
            "https://callback.example.com/callback".to_string(),
            "https://claude.ai/api/mcp/auth_callback".to_string(),
        ]
    );
    assert!(!cfg.mcp.auth.disable_static_token_with_oauth);
}

#[test]
fn oauth_mode_rejects_missing_public_url() {
    let mut cfg = loopback_config_with_token();
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.google_client_id = Some("id".into());
    cfg.mcp.auth.google_client_secret = Some("secret".into());
    cfg.mcp.auth.admin_email = "admin@example.com".into();

    let err = validate_auth_config(&cfg, true).unwrap_err();
    assert!(err.to_string().contains("CORTEX_PUBLIC_URL"));
}

#[test]
fn oauth_mode_rejects_missing_google_client_id() {
    let mut cfg = loopback_config_with_token();
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.public_url = Some("https://syslog.example.com".into());
    cfg.mcp.auth.google_client_secret = Some("secret".into());
    cfg.mcp.auth.admin_email = "admin@example.com".into();

    let err = validate_auth_config(&cfg, true).unwrap_err();
    assert!(err.to_string().contains("CORTEX_GOOGLE_CLIENT_ID"));
}

#[test]
fn oauth_mode_rejects_missing_google_client_secret() {
    let mut cfg = loopback_config_with_token();
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.public_url = Some("https://syslog.example.com".into());
    cfg.mcp.auth.google_client_id = Some("id".into());
    cfg.mcp.auth.admin_email = "admin@example.com".into();

    let err = validate_auth_config(&cfg, true).unwrap_err();
    assert!(err.to_string().contains("CORTEX_GOOGLE_CLIENT_SECRET"));
}

#[test]
fn oauth_mode_rejects_empty_allowlist_and_admin() {
    let mut cfg = loopback_config_with_token();
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.public_url = Some("https://syslog.example.com".into());
    cfg.mcp.auth.google_client_id = Some("id".into());
    cfg.mcp.auth.google_client_secret = Some("secret".into());
    // Both empty.
    cfg.mcp.auth.allowed_emails.clear();
    cfg.mcp.auth.admin_email.clear();

    let err = validate_auth_config(&cfg, true).unwrap_err();
    assert!(
        err.to_string().contains("admin_email"),
        "wrong error: {err}"
    );
}

#[test]
fn oauth_mode_rejects_allowed_emails_without_admin_until_enforced() {
    let mut cfg = loopback_config_with_token();
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.public_url = Some("https://syslog.example.com".into());
    cfg.mcp.auth.google_client_id = Some("id".into());
    cfg.mcp.auth.google_client_secret = Some("secret".into());
    cfg.mcp.auth.allowed_emails = vec!["admin@example.com".into()];

    let err = validate_auth_config(&cfg, true).unwrap_err();
    assert!(
        err.to_string().contains("allowed_emails"),
        "wrong error: {err}"
    );
}

#[test]
fn oauth_mode_accepts_admin_email_alone() {
    let mut cfg = loopback_config_with_token();
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.public_url = Some("https://syslog.example.com".into());
    cfg.mcp.auth.google_client_id = Some("id".into());
    cfg.mcp.auth.google_client_secret = Some("secret".into());
    cfg.mcp.auth.admin_email = "admin@example.com".into();

    validate_auth_config(&cfg, true).expect("admin_email alone counts as allowlist");
}

#[test]
fn oauth_mode_rejects_allowed_emails_even_with_admin_until_enforced() {
    let mut cfg = loopback_config_with_token();
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.public_url = Some("https://syslog.example.com".into());
    cfg.mcp.auth.google_client_id = Some("id".into());
    cfg.mcp.auth.google_client_secret = Some("secret".into());
    cfg.mcp.auth.admin_email = "admin@example.com".into();
    cfg.mcp.auth.allowed_emails = vec!["ops@example.com".into()];

    let err = validate_auth_config(&cfg, true).unwrap_err();
    assert!(
        err.to_string().contains("allowed_emails"),
        "wrong error: {err}"
    );
}

#[test]
fn bearer_and_oauth_can_coexist() {
    // Static token + OAuth fully configured = both pass validation.
    let mut cfg = loopback_config_with_token();
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.public_url = Some("https://syslog.example.com".into());
    cfg.mcp.auth.google_client_id = Some("id".into());
    cfg.mcp.auth.google_client_secret = Some("secret".into());
    cfg.mcp.auth.admin_email = "admin@example.com".into();

    validate_auth_config(&cfg, true).expect("bearer + oauth coexistence");
    assert!(cfg.mcp.api_token.is_some());
    assert_eq!(cfg.mcp.auth.mode, AuthMode::OAuth);
}

#[test]
fn loopback_bind_with_no_auth_is_permitted() {
    let mut cfg = Config::default();
    cfg.mcp.host = "127.0.0.1".into();
    cfg.mcp.api_token = None;
    validate_auth_config(&cfg, true).expect("loopback dev mode");
}

#[test]
fn explicit_no_auth_rejects_non_loopback_bind_without_trusted_gateway() {
    let mut cfg = Config::default();
    cfg.mcp.host = "0.0.0.0".into();
    cfg.mcp.api_token = None;
    cfg.mcp.no_auth = true;
    let err = validate_auth_config(&cfg, true)
        .expect_err("non-loopback no_auth must require trusted gateway flag");
    assert!(err.to_string().contains("CORTEX_TRUSTED_GATEWAY_NO_AUTH"));
}

#[test]
fn explicit_trusted_gateway_no_auth_allows_non_loopback_bind_without_token() {
    let mut cfg = Config::default();
    cfg.mcp.host = "0.0.0.0".into();
    cfg.mcp.api_token = None;
    cfg.mcp.no_auth = true;
    cfg.mcp.trusted_gateway_no_auth = true;
    validate_auth_config(&cfg, true).expect("gateway-protected no-auth mode");
}

#[test]
fn explicit_no_auth_ignores_stale_oauth_fields() {
    let mut cfg = Config::default();
    cfg.mcp.host = "0.0.0.0".into();
    cfg.mcp.api_token = None;
    cfg.mcp.no_auth = true;
    cfg.mcp.trusted_gateway_no_auth = true;
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.allowed_emails = vec!["stale@example.com".into()];

    validate_auth_config(&cfg, true).expect("no_auth bypasses unused OAuth config");
}

#[test]
fn loopback_variants_pass_safety_gate() {
    for host in ["127.0.0.1", "::1", "127.0.0.5"] {
        let mut cfg = Config::default();
        cfg.mcp.host = host.into();
        cfg.mcp.api_token = None;
        validate_auth_config(&cfg, true)
            .unwrap_or_else(|err| panic!("{host} should be loopback: {err}"));
    }
}

#[test]
fn non_loopback_bind_without_auth_bails() {
    for host in ["0.0.0.0", "::", "localhost", "myhost.example.com"] {
        let mut cfg = Config::default();
        cfg.mcp.host = host.into();
        cfg.mcp.api_token = None;
        let err = validate_auth_config(&cfg, true)
            .err()
            .unwrap_or_else(|| panic!("{host} must be rejected without auth"));
        let msg = err.to_string();
        assert!(
            msg.contains("not a loopback") || msg.contains("loopback"),
            "wrong error for {host}: {msg}"
        );
    }
}

#[test]
fn non_loopback_bind_with_static_token_passes() {
    let mut cfg = Config::default();
    cfg.mcp.host = "0.0.0.0".into();
    cfg.mcp.api_token = Some("token".into());
    validate_auth_config(&cfg, true).expect("static token unlocks non-loopback bind");
}

#[test]
fn non_loopback_bind_with_oauth_and_static_token_passes() {
    let mut cfg = valid_oauth_config_without_token();
    cfg.mcp.host = "0.0.0.0".into();
    cfg.mcp.api_token = Some("token".into());
    validate_auth_config(&cfg, true).expect("oauth unlocks non-loopback bind");
}

#[test]
fn non_loopback_oauth_without_static_token_rejects_otlp_write_exposure() {
    let mut cfg = valid_oauth_config_without_token();
    cfg.mcp.host = "0.0.0.0".into();

    let err = validate_auth_config(&cfg, true).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("OTLP /v1/logs") && msg.contains("CORTEX_TOKEN"),
        "wrong error: {msg}"
    );
}

#[test]
fn loopback_oauth_without_static_token_keeps_dev_mode_allowed() {
    let mut cfg = valid_oauth_config_without_token();
    cfg.mcp.host = "127.0.0.1".into();
    validate_auth_config(&cfg, true).expect("loopback OTLP exposure is local-only");
}

#[test]
#[serial]
fn auth_mode_parses_lowercase_only() {
    let mut mode = AuthMode::Bearer;
    std::env::set_var("__TEST_AUTH_MODE_PARSE", "OAUTH");
    env_override_auth_mode("__TEST_AUTH_MODE_PARSE", &mut mode).unwrap();
    std::env::remove_var("__TEST_AUTH_MODE_PARSE");
    assert_eq!(mode, AuthMode::OAuth, "case-insensitive");
}

#[test]
fn auth_toml_section_parses() {
    let raw = r#"
        [mcp.auth]
        admin_email = "admin@example.com"
        allowed_emails = ["admin@example.com", "ops@example.com"]
        sqlite_path = "custom-auth.db"
        key_path = "custom-key.pem"
        access_token_ttl_secs = 1800
        refresh_token_ttl_secs = 14400
        auth_code_ttl_secs = 120
        register_rpm = 5
        authorize_rpm = 30
        disable_static_token_with_oauth = false
        allowed_client_redirect_uris = ["https://claude.ai/api/mcp/auth_callback"]
    "#;
    let cfg: Config = toml::from_str(raw).expect("auth section should parse");
    let auth = &cfg.mcp.auth;
    assert_eq!(auth.admin_email, "admin@example.com");
    assert_eq!(auth.allowed_emails.len(), 2);
    assert_eq!(auth.sqlite_path, std::path::PathBuf::from("custom-auth.db"));
    assert_eq!(auth.key_path, std::path::PathBuf::from("custom-key.pem"));
    assert_eq!(auth.access_token_ttl_secs, 1_800);
    assert_eq!(auth.refresh_token_ttl_secs, 14_400);
    assert_eq!(auth.auth_code_ttl_secs, 120);
    assert_eq!(auth.register_rpm, 5);
    assert_eq!(auth.authorize_rpm, 30);
    assert!(!auth.disable_static_token_with_oauth);
    assert_eq!(
        auth.allowed_client_redirect_uris,
        vec!["https://claude.ai/api/mcp/auth_callback".to_string()]
    );
}

#[test]
fn repo_local_config_uses_repo_local_db_path() {
    let cfg: Config =
        toml::from_str(include_str!("../config.toml")).expect("repo config.toml should parse");
    assert_eq!(
        cfg.storage.db_path,
        std::path::PathBuf::from("data/cortex.db"),
        "repo config.toml should use a writable repo-local DB path for local dev"
    );
}

#[test]
fn repo_local_oauth_config_rejects_allowed_emails_until_enforced() {
    let mut cfg: Config =
        toml::from_str(include_str!("../config.toml")).expect("repo config.toml should parse");
    cfg.mcp.auth.mode = AuthMode::OAuth;
    cfg.mcp.auth.public_url = Some("https://syslog.example.com".into());
    cfg.mcp.auth.google_client_id = Some("id".into());
    cfg.mcp.auth.google_client_secret = Some("secret".into());
    cfg.mcp.auth.admin_email = "admin@example.com".into();
    cfg.mcp.auth.allowed_emails = vec!["ops@example.com".into()];

    let err = validate_auth_config(&cfg, true).unwrap_err();
    assert!(
        err.to_string().contains("allowed_emails"),
        "wrong error: {err}"
    );
}

// ---- syslog-mcp-w4hh: storage budget defaults + validation ----

/// The self-wipe stop-the-bleed: min_free_disk_mb defaults to 0 so cortex does
/// not treat external whole-FS pressure as a trigger to delete its own data.
#[test]
fn min_free_disk_mb_default_is_zero() {
    let storage = StorageConfig::default();
    assert_eq!(
        storage.min_free_disk_mb, 0,
        "min_free_disk_mb must default to 0 (no external-pressure self-wipe)"
    );
}

/// W2 MUST-FIX: StorageConfig::default() must PASS validate_storage_config.
/// validate_storage_config rejects recovery_free_disk_mb != 0 when
/// min_free_disk_mb == 0, so default_recovery_free_disk_mb must also be 0 — else
/// fresh deploys crash at startup. StorageConfig::for_test uses 0/0 and so cannot
/// catch this; this test asserts the real Default impl.
#[test]
fn default_storage_config_passes_validation() {
    let storage = StorageConfig::default();
    assert_eq!(
        storage.recovery_free_disk_mb, 0,
        "recovery_free_disk_mb must default to 0 to pair with min_free_disk_mb=0"
    );
    validate_storage_config(&storage)
        .expect("StorageConfig::default() must pass validate_storage_config (W2)");
}

/// A TOML config with no [storage] overrides must also deserialize to defaults
/// that pass validation — guards the serde-default path, not just Default::default.
#[test]
fn default_toml_storage_config_passes_validation() {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(default)]
        storage: StorageConfig,
    }
    let parsed: Wrapper = toml::from_str("").expect("empty config must deserialize");
    validate_storage_config(&parsed.storage)
        .expect("default-deserialized StorageConfig must pass validation (W2)");
}

/// The err+ floor invariant: a window with a zero per-source cap is rejected.
#[test]
fn err_floor_window_with_zero_cap_is_rejected() {
    let storage = StorageConfig {
        err_floor_window_hours: 24,
        err_floor_per_source_cap: 0,
        ..StorageConfig::default()
    };
    let err = validate_storage_config(&storage).unwrap_err();
    assert!(
        err.to_string().contains("err_floor_per_source_cap"),
        "wrong error: {err}"
    );
}
