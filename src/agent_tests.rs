use super::*;

#[test]
fn syslog_target_from_heartbeat_extracts_host_and_uses_syslog_port() {
    assert_eq!(
        AgentStreamsConfig::syslog_target_from_heartbeat("http://dookie:3100"),
        Some("dookie:1514".to_string())
    );
    assert_eq!(
        AgentStreamsConfig::syslog_target_from_heartbeat("https://cortex.example.test:3100/mcp/"),
        Some("cortex.example.test:1514".to_string())
    );
}

#[test]
fn syslog_target_from_heartbeat_rejects_non_http_or_missing_host() {
    assert_eq!(
        AgentStreamsConfig::syslog_target_from_heartbeat("dookie:3100"),
        None
    );
    assert_eq!(
        AgentStreamsConfig::syslog_target_from_heartbeat("https:///mcp"),
        None
    );
}

#[tokio::test]
async fn run_agent_streams_returns_immediately_when_all_sources_disabled() {
    let config = AgentStreamsConfig {
        docker: false,
        docker_url: "unix:///var/run/docker.sock".to_string(),
        journald: false,
        syslog_file: None,
        file_tails: Vec::new(),
        syslog_target: "127.0.0.1:1514".to_string(),
        hostname: "test-host".to_string(),
    };

    run_agent_streams(config).await.unwrap();
}

#[test]
fn parse_file_tails_extracts_path_and_tag_pairs() {
    use syslog_file::parse_file_tails;
    let parsed = parse_file_tails(
        "/mnt/appdata/adguard/var/data/querylog.json:adguard-query, /log/access.log:swag-access",
    );
    assert_eq!(parsed.len(), 2);
    assert_eq!(
        parsed[0].path,
        PathBuf::from("/mnt/appdata/adguard/var/data/querylog.json")
    );
    assert_eq!(parsed[0].tag.as_deref(), Some("adguard-query"));
    assert_eq!(parsed[1].path, PathBuf::from("/log/access.log"));
    assert_eq!(parsed[1].tag.as_deref(), Some("swag-access"));

    // Entries without a usable :TAG are skipped; empties ignored.
    assert!(parse_file_tails("/no/tag/here").is_empty());
    assert!(parse_file_tails(" , ").is_empty());
}
