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
    assert!(COMPOSE_ASSET.contains("syslog-setup-build-stanza-start"));
    assert!(COMPOSE_ASSET.contains("syslog-setup-build-stanza-end"));
    let compose = installed_compose_asset();
    assert_ne!(compose, COMPOSE_ASSET);
    assert!(compose.contains("image: ghcr.io/jmagar/syslog-mcp:"));
    assert!(!compose.contains("syslog-setup-build-stanza-start"));
    assert!(!compose.contains("syslog-setup-build-stanza-end"));
    assert!(!compose.contains("\n    build:\n"));
    assert!(!compose.contains("dockerfile: config/Dockerfile"));
    assert!(compose.contains("      - path: ../.env\n"));
}

#[test]
fn ai_index_timer_script_uses_host_syslog_and_disables_docker_ingest() {
    let script = ai_index_script();
    assert!(script.contains("command -v syslog"));
    assert!(script.contains("syslog --version"));
    assert!(script.contains("syslog ai index --json"));
    assert!(script.contains("SYSLOG_DOCKER_INGEST_ENABLED"));
    assert!(script.contains(".claude/plugins/data/syslog-jmagar-lab/syslog.db"));
}

#[test]
fn ai_index_timer_units_are_host_user_units() {
    let unit = ai_index_service_unit(std::path::Path::new("/home/me/.local/bin/syslog-ai-index"));
    let timer = ai_index_timer_unit();

    assert!(unit.contains("Description=syslog-mcp local AI transcript index"));
    assert!(unit.contains("ExecStart=/home/me/.local/bin/syslog-ai-index"));
    assert!(timer.contains("OnUnitActiveSec=30min"));
    assert!(timer.contains("WantedBy=timers.target"));
}

#[test]
fn ai_watch_env_file_pins_db_and_disables_docker_ingest() {
    let env = ai_watch_env_file(std::path::Path::new("/home/me/syslog.db"));
    assert!(env.contains("SYSLOG_MCP_DB_PATH=/home/me/syslog.db"));
    assert!(env.contains("SYSLOG_DOCKER_INGEST_ENABLED=false"));
    assert!(env.contains("RUST_LOG=warn"));
}

#[test]
fn ai_watch_service_unit_is_hardened_and_uses_absolute_exec() {
    let unit = ai_watch_service_unit(
        std::path::Path::new("/home/me/.local/bin/syslog"),
        std::path::Path::new("/home/me/.config/syslog-mcp/ai-watch.env"),
        std::path::Path::new("/home/me/.syslog-mcp/data/syslog.db"),
        std::path::Path::new("/home/me/.local/state/syslog-mcp"),
        std::path::Path::new("/home/me"),
    );

    assert!(unit.contains("Type=simple"));
    assert!(unit.contains("EnvironmentFile=/home/me/.config/syslog-mcp/ai-watch.env"));
    assert!(unit.contains(
        "Environment=PATH=/home/me/.local/bin:/home/me/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
    ));
    assert!(
        unit.contains("Environment=CARGO_TARGET_DIR=/home/me/.local/state/syslog-mcp/cargo-target")
    );
    assert!(unit.contains("WorkingDirectory=/"));
    assert!(unit.contains("ExecStart=/home/me/.local/bin/syslog ai watch --no-initial-scan --json"));
    assert!(unit.contains("Restart=on-failure"));
    assert!(unit.contains("StartLimitBurst=5"));
    assert!(unit.contains("UMask=0077"));
    assert!(unit.contains("NoNewPrivileges=true"));
    assert!(unit.contains("PrivateTmp=true"));
    assert!(unit.contains("ProtectSystem=strict"));
    assert!(unit.contains("ProtectHome=read-only"));
    assert!(unit.contains("BindReadOnlyPaths=-/home/me/.claude/projects -/home/me/.codex/sessions"));
    assert!(unit.contains("BindPaths=/home/me/.syslog-mcp/data /home/me/.local/state/syslog-mcp"));
    assert!(
        unit.contains("ReadWritePaths=/home/me/.syslog-mcp/data /home/me/.local/state/syslog-mcp")
    );
    assert!(unit.contains("WantedBy=default.target"));
}

#[test]
fn setup_path_value_rejects_unit_breaking_characters() {
    assert!(setup_path_value(std::path::Path::new("/home/me/syslog.db")).is_ok());
    assert!(setup_path_value(std::path::Path::new("/home/me/bad path/syslog.db")).is_err());
    assert!(setup_path_value(std::path::Path::new("/home/me/%n/syslog.db")).is_err());
}

#[test]
fn db_path_from_setup_env_uses_absolute_compose_data_volume() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    std::fs::write(
        &env_path,
        "SYSLOG_MCP_DB_PATH=/data/syslog.db\nSYSLOG_MCP_DATA_VOLUME=/srv/syslog-data\n",
    )
    .unwrap();

    assert_eq!(
        db_path_from_setup_env(&env_path).unwrap(),
        Some(std::path::PathBuf::from("/srv/syslog-data/syslog.db"))
    );
}

#[test]
fn db_path_from_setup_env_rejects_container_db_without_absolute_data_volume() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    std::fs::write(
        &env_path,
        "SYSLOG_MCP_DB_PATH=/data/syslog.db\nSYSLOG_MCP_DATA_VOLUME=syslog-data\n",
    )
    .unwrap();

    let err = db_path_from_setup_env(&env_path).unwrap_err();
    assert!(err
        .to_string()
        .contains("SYSLOG_MCP_DATA_VOLUME is not absolute"));
}

#[test]
fn validate_executable_path_rejects_debug_build_paths_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join(".cache/cargo/debug/syslog");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, "#!/bin/sh\n").unwrap();

    let err = validate_executable_path(bin).unwrap_err();
    assert!(err.to_string().contains("debug/worktree binary"));
}

#[test]
fn validate_db_path_rejects_relative_without_creating_parent() {
    let relative_dir = format!(
        "relative-db-dir-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let relative = std::path::PathBuf::from(&relative_dir).join("syslog.db");
    let err = validate_db_path(relative).unwrap_err();
    assert!(err.to_string().contains("must be absolute"));
    assert!(!std::path::Path::new(&relative_dir).exists());
}

#[cfg(unix)]
#[test]
fn private_and_executable_writers_reject_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let private_link = dir.path().join("private-link");
    let exec_link = dir.path().join("exec-link");
    std::fs::write(&target, "keep").unwrap();
    std::os::unix::fs::symlink(&target, &private_link).unwrap();
    std::os::unix::fs::symlink(&target, &exec_link).unwrap();

    assert!(write_private_file(&private_link, "secret").is_err());
    assert!(write_executable_file(&exec_link, "#!/bin/sh\n").is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
}

#[test]
fn ai_watch_service_content_phase_detects_stale_unit() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join("ai-watch.env");
    let service_path = dir.path().join("syslog-ai-watch.service");
    let state_dir = dir.path().join("state");
    let db_path = dir.path().join("data/syslog.db");
    let user_home = dir.path().join("home");
    let bin = dir.path().join("bin/syslog");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, "#!/bin/sh\n").unwrap();
    std::fs::write(&env_path, ai_watch_env_file(&db_path)).unwrap();
    std::fs::write(&service_path, "stale unit").unwrap();

    let phase = check_ai_watch_service_content_phase(
        &env_path,
        &service_path,
        &state_dir,
        &bin,
        &db_path,
        &user_home,
    );

    assert!(matches!(phase.status, SetupStatus::Error));
    assert!(phase
        .detail
        .contains("does not match generated AI watch unit"));
}

#[test]
fn debug_wrapper_script_builds_current_repo_debug_binary() {
    let script = debug_wrapper_script(std::path::Path::new("/home/me/workspace/syslog-mcp"));

    assert!(script.contains(r#"repo="${SYSLOG_MCP_REPO:-/home/me/workspace/syslog-mcp}""#));
    assert!(script.contains(r#"repo="${HOME}/workspace/syslog-mcp""#));
    assert!(script.contains(r#"export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-.cache/cargo}""#));
    assert!(script.contains("SYSLOG_DOCKER_INGEST_ENABLED"));
    assert!(script.contains("SYSLOG_MCP_AUTH_MODE"));
    assert!(script.contains("cargo build --quiet --bin syslog"));
    assert!(script.contains(r#"exec "${CARGO_TARGET_DIR}/debug/syslog" "$@""#));
}

#[test]
fn debug_wrapper_content_phase_detects_stale_wrapper() {
    let dir = tempfile::tempdir().unwrap();
    let wrapper = dir.path().join("syslog");
    std::fs::write(&wrapper, "#!/bin/sh\nexec old\n").unwrap();

    let phase = check_debug_wrapper_content_phase(&wrapper, dir.path());

    assert!(matches!(phase.status, SetupStatus::Error));
    assert!(phase
        .detail
        .contains("does not match generated debug wrapper"));
}

#[test]
fn transcript_root_permissions_phase_reports_missing_roots() {
    let dir = tempfile::tempdir().unwrap();

    let phase = transcript_root_permissions_phase(dir.path());

    assert!(matches!(phase.status, SetupStatus::Error));
    assert!(phase.detail.contains(".claude/projects"));
    assert!(phase.detail.contains(".codex/sessions"));
}

#[test]
fn summarize_ai_index_output_reports_key_counts() {
    let summary = summarize_ai_index_output(
        r#"{
  "discovered_files": 3,
  "ingested": 2,
  "skipped_dupes": 1,
  "parse_errors": 4,
  "storage_blocked_chunks": 0,
  "file_errors": ["bad"]
}"#,
    );

    assert_eq!(
        summary,
        "indexed files=3 ingested=2 duplicates=1 parse_errors=4 storage_blocked=0 dropped_metadata_fields=0 file_errors=1"
    );
}

#[test]
fn ai_index_output_has_failures_detects_retryable_failures() {
    assert!(!ai_index_output_has_failures(
        r#"{"parse_errors":0,"storage_blocked_chunks":0,"file_errors":[]}"#
    ));
    assert!(ai_index_output_has_failures(
        r#"{"parse_errors":1,"storage_blocked_chunks":0,"file_errors":[]}"#
    ));
    assert!(ai_index_output_has_failures(
        r#"{"parse_errors":0,"storage_blocked_chunks":1,"file_errors":[]}"#
    ));
    assert!(ai_index_output_has_failures(
        r#"{"parse_errors":0,"storage_blocked_chunks":0,"file_errors":["bad"]}"#
    ));
    assert!(ai_index_output_has_failures(
        r#"{"parse_errors":0,"storage_blocked_chunks":0,"dropped_metadata_fields":1,"file_errors":[]}"#
    ));
}

#[test]
fn inferred_user_bus_env_uses_runtime_bus_when_present() {
    let Some((runtime_dir, bus_address)) = inferred_user_bus_env() else {
        return;
    };

    assert!(runtime_dir.is_absolute());
    assert!(bus_address.starts_with("unix:path="));
    assert!(bus_address.ends_with("/bus"));
}
