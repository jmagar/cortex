use super::*;

#[test]
fn ensure_env_file_preserves_existing_token_and_adds_compose_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(&env_path, "SYSLOG_MCP_TOKEN=keep-me\n").unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();
    let raw = std::fs::read_to_string(&env_path).unwrap();

    assert_eq!(
        result.values.get("SYSLOG_MCP_TOKEN").map(String::as_str),
        Some("keep-me")
    );
    assert!(raw.contains("SYSLOG_MCP_TOKEN=keep-me"));
    assert!(raw.contains("SYSLOG_MCP_DATA_VOLUME="));
    assert!(raw.contains("SYSLOG_MCP_DB_PATH=/data/syslog.db"));
}

#[test]
fn parse_env_ignores_comments_and_blank_lines() {
    let parsed = parse_env("\n# comment\nA=1\nB = two\n");
    assert_eq!(parsed.get("A").map(String::as_str), Some("1"));
    assert_eq!(parsed.get("B").map(String::as_str), Some("two"));
}

#[test]
fn installed_compose_asset_uses_published_image_only() {
    let compose = installed_compose_asset();
    assert!(compose.contains("image: ghcr.io/jmagar/syslog-mcp:"));
    assert!(!compose.contains("\n    build:\n"));
    assert!(!compose.contains("dockerfile: config/Dockerfile"));
    assert!(compose.contains("      - path: ../.env\n"));
}
