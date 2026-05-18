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
    assert!(raw.contains("COMPOSE_PROJECT_NAME=syslog-jmagar-lab"));
}

/// `SYSLOG_API_TOKEN` is always provisioned (API is unconditionally mounted).
/// First run on a clean .env generates a 64-char hex token (32 bytes hex-encoded).
#[test]
fn ensure_env_file_generates_api_token_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    let token = result
        .values
        .get("SYSLOG_API_TOKEN")
        .expect("SYSLOG_API_TOKEN must be present after ensure_env_file");
    assert!(
        token.len() >= 32,
        "generated SYSLOG_API_TOKEN must be at least 32 chars, got {} chars",
        token.len()
    );
    assert!(
        token.chars().all(|c| c.is_ascii_hexdigit()),
        "generated SYSLOG_API_TOKEN must be hex"
    );
    let raw = std::fs::read_to_string(&env_path).unwrap();
    assert!(raw.contains(&format!("SYSLOG_API_TOKEN={token}")));
}

/// Re-running `ensure_env_file` against a .env that already has a
/// SYSLOG_API_TOKEN must preserve it byte-for-byte — mirrors the
/// SYSLOG_MCP_TOKEN contract that operators depend on for token rotation.
#[test]
fn ensure_env_file_preserves_existing_api_token_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(
        &env_path,
        "SYSLOG_MCP_TOKEN=mcp-keep\nSYSLOG_API_TOKEN=api-keep-me-exactly\n",
    )
    .unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    assert_eq!(
        result.values.get("SYSLOG_API_TOKEN").map(String::as_str),
        Some("api-keep-me-exactly")
    );
    let raw = std::fs::read_to_string(&env_path).unwrap();
    assert!(raw.contains("SYSLOG_API_TOKEN=api-keep-me-exactly"));
}

/// Cutover (bead 0p8r.10): on first install, `ensure_env_file` writes
/// `SYSLOG_USE_HTTP=true` so the CLI defaults to HTTP transport via the
/// container REST API.
#[test]
fn ensure_env_file_sets_use_http_true_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    assert_eq!(
        result.values.get("SYSLOG_USE_HTTP").map(String::as_str),
        Some("true"),
        "first install must default SYSLOG_USE_HTTP=true"
    );
    let raw = std::fs::read_to_string(&env_path).unwrap();
    assert!(
        raw.contains("SYSLOG_USE_HTTP=true"),
        "rendered .env must contain SYSLOG_USE_HTTP=true literally"
    );
}

/// Operator opt-out (`SYSLOG_USE_HTTP=false`) must survive `setup repair`
/// byte-for-byte — unlike SYSLOG_API_TOKEN, this is a behaviour toggle the
/// operator may legitimately disable.
#[test]
fn ensure_env_file_preserves_use_http_false_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(&env_path, "SYSLOG_USE_HTTP=false\n").unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    assert_eq!(
        result.values.get("SYSLOG_USE_HTTP").map(String::as_str),
        Some("false"),
        "operator override SYSLOG_USE_HTTP=false must be preserved exactly"
    );
    let raw = std::fs::read_to_string(&env_path).unwrap();
    assert!(raw.contains("SYSLOG_USE_HTTP=false"));
    assert!(!raw.contains("SYSLOG_USE_HTTP=true"));
}

/// Idempotent re-run: when `SYSLOG_USE_HTTP=true` already exists, repair
/// leaves it untouched (no double-write, no value rewrite).
#[test]
fn ensure_env_file_preserves_use_http_true_on_rerun() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(&env_path, "SYSLOG_USE_HTTP=true\n").unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    assert_eq!(
        result.values.get("SYSLOG_USE_HTTP").map(String::as_str),
        Some("true")
    );
    let raw = std::fs::read_to_string(&env_path).unwrap();
    // Exactly one occurrence — write_env must not duplicate the key.
    assert_eq!(
        raw.matches("SYSLOG_USE_HTTP=").count(),
        1,
        "SYSLOG_USE_HTTP must appear exactly once after re-run"
    );
}

/// Empty value (`SYSLOG_USE_HTTP=`) is treated as an explicit operator
/// choice and preserved. The CLI falls through to its compiled default in
/// that case; we do NOT silently rewrite to `true` because doing so would
/// override an operator who wrote the line intentionally blank to "let the
/// binary decide". Mirrors the wider design: operator intent always wins
/// for behaviour toggles.
#[test]
fn ensure_env_file_preserves_empty_use_http_value() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(&env_path, "SYSLOG_USE_HTTP=\n").unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    assert_eq!(
        result.values.get("SYSLOG_USE_HTTP").map(String::as_str),
        Some(""),
        "empty SYSLOG_USE_HTTP must be preserved (operator intent wins)"
    );
}

/// Blank API token must be replaced (an empty value would still fail the
/// runtime `api.rs` token check and brick container startup).
#[test]
fn ensure_env_file_replaces_blank_api_token() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(&env_path, "SYSLOG_API_TOKEN=\n").unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    let token = result.values.get("SYSLOG_API_TOKEN").unwrap();
    assert!(!token.trim().is_empty());
    assert!(token.len() >= 32);
}

/// Atomic `write_env`: the live target file is either fully written or
/// unchanged. Crucial because a corrupt .env bricks container startup
/// (api.rs bails on empty SYSLOG_API_TOKEN).
#[test]
fn write_env_replaces_file_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    std::fs::write(&env_path, "OLD=value\n").unwrap();

    let mut env = std::collections::BTreeMap::new();
    env.insert("NEW".to_string(), "fresh".to_string());
    env.insert("SECRET".to_string(), "shhh".to_string());
    write_env(&env_path, &env).unwrap();

    let raw = std::fs::read_to_string(&env_path).unwrap();
    assert!(raw.contains("NEW=fresh"));
    assert!(raw.contains("SECRET=shhh"));
    assert!(!raw.contains("OLD=value"));

    // No orphaned tempfiles in the directory after a successful write.
    for entry in std::fs::read_dir(dir.path()).unwrap().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        assert!(
            !name.starts_with(".env.tmp."),
            "orphaned tempfile after write_env: {name}"
        );
    }
}

/// Permission check (Unix only): atomic write must still produce a 0o600 file.
#[cfg(unix)]
#[test]
fn write_env_sets_0600_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let mut env = std::collections::BTreeMap::new();
    env.insert("K".to_string(), "v".to_string());
    write_env(&env_path, &env).unwrap();

    let mode = std::fs::metadata(&env_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "expected 0o600 perms, got {mode:o}");
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

#[test]
fn validate_db_path_rejects_root_parent_and_unit_breaking_chars() {
    let root_db = std::path::PathBuf::from("/syslog.db");
    let err = validate_db_path(root_db).unwrap_err();
    assert!(err.to_string().contains("non-root directory"));

    let spaced = std::path::PathBuf::from("/tmp/syslog mcp/syslog.db");
    let err = validate_db_path(spaced).unwrap_err();
    assert!(err.to_string().contains("unsupported character"));
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
fn debug_compose_override_builds_local_debug_image_from_repo() {
    let override_yaml =
        debug_compose_override(std::path::Path::new("/home/me/workspace/syslog-mcp"));

    assert!(override_yaml.contains("image: syslog-mcp:local-debug"));
    assert!(override_yaml.contains("context: /home/me/workspace/syslog-mcp"));
    assert!(override_yaml.contains("dockerfile: config/Dockerfile"));
    assert!(override_yaml.contains("SYSLOG_BUILD_PROFILE: debug"));
}

#[test]
fn debug_compose_content_phase_detects_stale_override() {
    let dir = tempfile::tempdir().unwrap();
    let override_path = dir.path().join("docker-compose.override.yml");
    std::fs::write(&override_path, "services: {}\n").unwrap();

    let phase = check_debug_compose_content_phase(&override_path, dir.path());

    assert!(matches!(phase.status, SetupStatus::Error));
    assert!(phase
        .detail
        .contains("does not match generated debug Compose override"));
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
        "indexed files=3 ingested=2 duplicates=1 parse_errors=4 storage_blocked=0 dropped_metadata_fields=0 file_errors=1; inspect with `syslog ai errors --limit 20`, `syslog ai checkpoints --errors`, then rerun `syslog ai index --json` after fixes"
    );
}

#[test]
fn ai_index_output_status_classifies_blocking_and_recoverable_failures() {
    assert_eq!(
        ai_index_output_status(r#"{"parse_errors":0,"storage_blocked_chunks":0,"file_errors":[]}"#),
        (SetupStatus::Ok, None)
    );
    assert_eq!(
        ai_index_output_status(r#"{"parse_errors":1,"storage_blocked_chunks":0,"file_errors":[]}"#),
        (SetupStatus::Warn, Some(SetupIssueKind::DataQualityWarning))
    );
    assert_eq!(
        ai_index_output_status(r#"{"parse_errors":0,"storage_blocked_chunks":1,"file_errors":[]}"#),
        (SetupStatus::Error, Some(SetupIssueKind::BlockingError))
    );
    assert_eq!(
        ai_index_output_status(
            r#"{"parse_errors":0,"storage_blocked_chunks":0,"file_errors":["bad"]}"#
        ),
        (SetupStatus::Warn, Some(SetupIssueKind::DataQualityWarning))
    );
    assert_eq!(
        ai_index_output_status(
            r#"{"parse_errors":0,"storage_blocked_chunks":0,"dropped_metadata_fields":1,"file_errors":[]}"#
        ),
        (SetupStatus::Warn, Some(SetupIssueKind::DataQualityWarning))
    );
    assert_eq!(
        ai_index_output_status("not json"),
        (SetupStatus::Error, Some(SetupIssueKind::BlockingError))
    );
}

#[test]
fn ai_watch_systemd_enable_gate_allows_data_quality_warnings() {
    let phases = vec![
        PhaseTimer::start("ai-watch-service-files").finish(SetupStatus::Ok, "ok"),
        PhaseTimer::start("ai-watch-initial-index").finish_with_issue(
            SetupStatus::Warn,
            Some(SetupIssueKind::DataQualityWarning),
            "parse_errors=1",
        ),
    ];
    assert!(!should_skip_ai_watch_systemd_enable(&phases));

    let mut phases_with_error = phases;
    phases_with_error.push(PhaseTimer::start("systemd").finish_with_issue(
        SetupStatus::Error,
        Some(SetupIssueKind::BlockingError),
        "systemctl failed",
    ));
    assert!(should_skip_ai_watch_systemd_enable(&phases_with_error));
}

#[test]
fn setup_report_exposes_ai_watch_summary_fields() {
    let phases = vec![
        PhaseTimer::start("ai-watch-initial-index").finish_with_issue(
            SetupStatus::Warn,
            Some(SetupIssueKind::DataQualityWarning),
            "parse_errors=1",
        ),
        PhaseTimer::start(AI_WATCH_SERVICE_ENABLED_PHASE).finish(SetupStatus::Ok, "enabled"),
        PhaseTimer::start(AI_WATCH_SERVICE_ACTIVE_PHASE).finish(SetupStatus::Error, "inactive"),
    ];
    let report = setup_report(
        SetupReportInput {
            mode: "ai-watch-service-check",
            elapsed_ms: 0,
            home: PathBuf::from("/tmp/home"),
            env_path: PathBuf::from("/tmp/home/.env"),
            compose_dir: PathBuf::from("/tmp/home/compose"),
            data_dir: PathBuf::from("/tmp/home/data"),
            health_url: "host-local helper".to_string(),
            mcp_url: "host-local helper".to_string(),
        },
        phases,
    );

    assert!(report.has_errors);
    assert_eq!(report.blocking_errors, 1);
    assert_eq!(report.data_quality_warnings, 1);
    assert_eq!(report.service_enabled, Some(true));
    assert_eq!(report.watcher_healthy, Some(false));
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
