use super::*;

#[test]
fn ensure_env_file_preserves_existing_token_and_adds_compose_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(&env_path, "CORTEX_TOKEN=keep-me\n").unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();
    let raw = std::fs::read_to_string(&env_path).unwrap();

    assert_eq!(
        result.values.get("CORTEX_TOKEN").map(String::as_str),
        Some("keep-me")
    );
    assert!(raw.contains("CORTEX_TOKEN=keep-me"));
    assert!(raw.contains("CORTEX_DATA_VOLUME="));
    assert!(raw.contains("CORTEX_DB_PATH=/data/cortex.db"));
    assert!(raw.contains("COMPOSE_PROJECT_NAME=syslog-jmagar-lab"));
}

/// `CORTEX_API_TOKEN` is always provisioned (API is unconditionally mounted).
/// First run on a clean .env generates a 64-char hex token (32 bytes hex-encoded).
#[test]
fn ensure_env_file_generates_api_token_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    let token = result
        .values
        .get("CORTEX_API_TOKEN")
        .expect("CORTEX_API_TOKEN must be present after ensure_env_file");
    // The implementation generates 32 random bytes encoded as hex → exactly
    // 64 ASCII chars. Lock down the precise length so future refactors that
    // shorten the token (and weaken brute-force resistance) fail this test.
    assert_eq!(
        token.len(),
        64,
        "generated CORTEX_API_TOKEN must be 64 hex chars (32 random bytes), got {} chars",
        token.len()
    );
    assert!(
        token.chars().all(|c| c.is_ascii_hexdigit()),
        "generated CORTEX_API_TOKEN must be hex"
    );
    let raw = std::fs::read_to_string(&env_path).unwrap();
    assert!(raw.contains(&format!("CORTEX_API_TOKEN={token}")));
}

/// Re-running `ensure_env_file` against a .env that already has a
/// CORTEX_API_TOKEN must preserve it byte-for-byte — mirrors the
/// CORTEX_TOKEN contract that operators depend on for token rotation.
#[test]
fn ensure_env_file_preserves_existing_api_token_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(
        &env_path,
        "CORTEX_TOKEN=mcp-keep\nCORTEX_API_TOKEN=api-keep-me-exactly\n",
    )
    .unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    assert_eq!(
        result.values.get("CORTEX_API_TOKEN").map(String::as_str),
        Some("api-keep-me-exactly")
    );
    let raw = std::fs::read_to_string(&env_path).unwrap();
    assert!(raw.contains("CORTEX_API_TOKEN=api-keep-me-exactly"));
}

/// Cutover (bead 0p8r.10): on first install, `ensure_env_file` writes
/// `CORTEX_USE_HTTP=true` so the CLI defaults to HTTP transport via the
/// container REST API.
#[test]
fn ensure_env_file_sets_use_http_true_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    assert_eq!(
        result.values.get("CORTEX_USE_HTTP").map(String::as_str),
        Some("true"),
        "first install must default CORTEX_USE_HTTP=true"
    );
    let raw = std::fs::read_to_string(&env_path).unwrap();
    assert!(
        raw.contains("CORTEX_USE_HTTP=true"),
        "rendered .env must contain CORTEX_USE_HTTP=true literally"
    );
}

/// Operator opt-out (`CORTEX_USE_HTTP=false`) must survive `setup repair`
/// byte-for-byte — unlike CORTEX_API_TOKEN, this is a behaviour toggle the
/// operator may legitimately disable.
#[test]
fn ensure_env_file_preserves_use_http_false_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(&env_path, "CORTEX_USE_HTTP=false\n").unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    assert_eq!(
        result.values.get("CORTEX_USE_HTTP").map(String::as_str),
        Some("false"),
        "operator override CORTEX_USE_HTTP=false must be preserved exactly"
    );
    let raw = std::fs::read_to_string(&env_path).unwrap();
    assert!(raw.contains("CORTEX_USE_HTTP=false"));
    assert!(!raw.contains("CORTEX_USE_HTTP=true"));
}

/// Idempotent re-run: when `CORTEX_USE_HTTP=true` already exists, repair
/// leaves it untouched (no double-write, no value rewrite).
#[test]
fn ensure_env_file_preserves_use_http_true_on_rerun() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(&env_path, "CORTEX_USE_HTTP=true\n").unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    assert_eq!(
        result.values.get("CORTEX_USE_HTTP").map(String::as_str),
        Some("true")
    );
    let raw = std::fs::read_to_string(&env_path).unwrap();
    // Exactly one occurrence — write_env must not duplicate the key.
    assert_eq!(
        raw.matches("CORTEX_USE_HTTP=").count(),
        1,
        "CORTEX_USE_HTTP must appear exactly once after re-run"
    );
}

/// Empty value (`CORTEX_USE_HTTP=`) is treated as an explicit operator
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
    std::fs::write(&env_path, "CORTEX_USE_HTTP=\n").unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    assert_eq!(
        result.values.get("CORTEX_USE_HTTP").map(String::as_str),
        Some(""),
        "empty CORTEX_USE_HTTP must be preserved (operator intent wins)"
    );
}

/// Blank API token must be replaced (an empty value would still fail the
/// runtime `api.rs` token check and brick container startup).
#[test]
fn ensure_env_file_replaces_blank_api_token() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let data_dir = dir.path().join("data");
    std::fs::write(&env_path, "CORTEX_API_TOKEN=\n").unwrap();

    let result = ensure_env_file(&env_path, &data_dir).unwrap();

    let token = result.values.get("CORTEX_API_TOKEN").unwrap();
    assert!(!token.trim().is_empty());
    assert!(token.len() >= 32);
}

/// Atomic `write_env`: the live target file is either fully written or
/// unchanged. Crucial because a corrupt .env bricks container startup
/// (api.rs bails on empty CORTEX_API_TOKEN).
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
        // write_env names its tempfile `.{file_name}.tmp.{pid}.{nanos}`.
        // For `.env` that's `..env.tmp.*`. Match the actual prefix so the
        // assertion catches real orphans instead of silently passing.
        assert!(
            !name.starts_with("..env.tmp."),
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
    // COMPOSE_ASSET is now docker-compose.prod.yml — the publishable template.
    assert!(COMPOSE_ASSET.contains("image: ghcr.io/jmagar/cortex:"));
    assert!(!COMPOSE_ASSET.contains("syslog-setup-build-stanza"));
    assert!(!COMPOSE_ASSET.contains("\n    build:\n"));

    let compose = installed_compose_asset();
    assert_ne!(compose, COMPOSE_ASSET);
    assert!(compose.contains("image: ghcr.io/jmagar/cortex:"));
    assert!(compose.contains("      - path: ../.env\n"));
    // env_file path is rewritten — source pattern must not survive.
    assert!(!compose.contains("      - path: .env\n"));
}

#[test]
fn installed_compose_asset_memory_limit_is_configurable_with_2g_default() {
    // Issue 3: the memory limit must default to 2G (was 512M, which OOM'd on
    // heavy stats queries) and be overridable via CORTEX_MEMORY_LIMIT so a
    // fresh deploy picks up the safer default and operators can tune it.
    assert!(
        COMPOSE_ASSET.contains("memory: ${CORTEX_MEMORY_LIMIT:-2G}"),
        "compose asset must use a configurable 2G memory limit"
    );
    assert!(
        !COMPOSE_ASSET.contains("memory: 512M"),
        "compose asset must not hardcode the old 512M limit"
    );
    // The installed transform must preserve the configurable limit.
    let compose = installed_compose_asset();
    assert!(compose.contains("memory: ${CORTEX_MEMORY_LIMIT:-2G}"));
}

#[test]
fn ai_index_timer_script_uses_host_cortex_and_disables_docker_ingest() {
    let script = ai_index_script();
    assert!(script.contains("command -v cortex"));
    assert!(script.contains("cortex --version"));
    assert!(script.contains("cortex ai index --json"));
    assert!(script.contains("CORTEX_DOCKER_INGEST_ENABLED"));
    assert!(script.contains(".claude/plugins/data/syslog-jmagar-lab/cortex.db"));
}

#[test]
fn ai_index_timer_units_are_host_user_units() {
    let unit = ai_index_service_unit(std::path::Path::new("/home/me/.local/bin/cortex-ai-index"));
    let timer = ai_index_timer_unit();

    assert!(unit.contains("Description=cortex local AI transcript index"));
    assert!(unit.contains("ExecStart=/home/me/.local/bin/cortex-ai-index"));
    assert!(timer.contains("OnUnitActiveSec=30min"));
    assert!(timer.contains("WantedBy=timers.target"));
}

#[test]
fn ai_watch_env_file_pins_db_and_disables_docker_ingest() {
    let env = ai_watch_env_file(std::path::Path::new("/home/me/cortex.db"));
    assert!(env.contains("CORTEX_DB_PATH=/home/me/cortex.db"));
    assert!(env.contains("CORTEX_DOCKER_INGEST_ENABLED=false"));
    assert!(env.contains("RUST_LOG=warn"));
}

#[test]
fn cortex_home_dir_can_be_inferred_from_user_local_bin_binary() {
    let exe = std::path::Path::new("/home/jmagar/.local/bin/cortex");

    assert_eq!(
        cortex_home_dir_from_exe_path(exe).as_deref(),
        Some(std::path::Path::new("/home/jmagar/.cortex"))
    );
}

#[test]
fn cortex_home_dir_can_be_inferred_from_user_workspace_binary() {
    let exe = std::path::Path::new("/home/jmagar/workspace/cortex/target/release/cortex");

    assert_eq!(
        cortex_home_dir_from_exe_path(exe).as_deref(),
        Some(std::path::Path::new("/home/jmagar/.cortex"))
    );
}

#[test]
fn cortex_home_dir_is_not_inferred_from_non_home_binary() {
    let exe = std::path::Path::new("/usr/local/bin/cortex");

    assert_eq!(cortex_home_dir_from_exe_path(exe), None);
}

#[test]
fn ai_watch_service_unit_is_hardened_and_uses_absolute_exec() {
    let unit = ai_watch_service_unit(
        std::path::Path::new("/home/me/.local/bin/cortex"),
        std::path::Path::new("/home/me/.config/cortex/ai-watch.env"),
        std::path::Path::new("/home/me/.cortex/data/cortex.db"),
        std::path::Path::new("/home/me/.local/state/cortex"),
        std::path::Path::new("/home/me"),
    );

    assert!(unit.contains("Type=simple"));
    assert!(unit.contains("EnvironmentFile=/home/me/.config/cortex/ai-watch.env"));
    assert!(unit.contains(
        "Environment=PATH=/home/me/.local/bin:/home/me/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
    ));
    assert!(unit.contains("Environment=CARGO_TARGET_DIR=/home/me/.local/state/cortex/cargo-target"));
    assert!(unit.contains("WorkingDirectory=/"));
    assert!(unit.contains("ExecStart=/home/me/.local/bin/cortex ai watch --no-initial-scan --json"));
    assert!(unit.contains("Restart=on-failure"));
    assert!(unit.contains("StartLimitBurst=5"));
    assert!(unit.contains("UMask=0077"));
    assert!(unit.contains("NoNewPrivileges=true"));
    assert!(unit.contains("PrivateTmp=true"));
    assert!(unit.contains("ProtectSystem=strict"));
    assert!(unit.contains("ProtectHome=read-only"));
    assert!(unit.contains("BindReadOnlyPaths=-/home/me/.claude/projects -/home/me/.codex/sessions"));
    assert!(unit.contains("BindPaths=/home/me/.cortex/data /home/me/.local/state/cortex"));
    assert!(unit.contains("ReadWritePaths=/home/me/.cortex/data /home/me/.local/state/cortex"));
    assert!(unit.contains("WantedBy=default.target"));
}

#[test]
fn setup_path_value_rejects_unit_breaking_characters() {
    assert!(setup_path_value(std::path::Path::new("/home/me/cortex.db")).is_ok());
    assert!(setup_path_value(std::path::Path::new("/home/me/bad path/cortex.db")).is_err());
    assert!(setup_path_value(std::path::Path::new("/home/me/%n/cortex.db")).is_err());
}

#[test]
fn db_path_from_setup_env_uses_absolute_compose_data_volume() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    std::fs::write(
        &env_path,
        "CORTEX_DB_PATH=/data/cortex.db\nCORTEX_DATA_VOLUME=/srv/syslog-data\n",
    )
    .unwrap();

    assert_eq!(
        db_path_from_setup_env(&env_path).unwrap(),
        Some(std::path::PathBuf::from("/srv/syslog-data/cortex.db"))
    );
}

#[test]
fn db_path_from_setup_env_rejects_container_db_without_absolute_data_volume() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    std::fs::write(
        &env_path,
        "CORTEX_DB_PATH=/data/cortex.db\nCORTEX_DATA_VOLUME=syslog-data\n",
    )
    .unwrap();

    let err = db_path_from_setup_env(&env_path).unwrap_err();
    assert!(err
        .to_string()
        .contains("CORTEX_DATA_VOLUME is not absolute"));
}

#[test]
fn validate_executable_path_rejects_debug_build_paths_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join(".cache/cargo/debug/cortex");
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
    let relative = std::path::PathBuf::from(&relative_dir).join("cortex.db");
    let err = validate_db_path(relative).unwrap_err();
    assert!(err.to_string().contains("must be absolute"));
    assert!(!std::path::Path::new(&relative_dir).exists());
}

#[test]
fn validate_db_path_rejects_root_parent_and_unit_breaking_chars() {
    let root_db = std::path::PathBuf::from("/cortex.db");
    let err = validate_db_path(root_db).unwrap_err();
    assert!(err.to_string().contains("non-root directory"));

    let spaced = std::path::PathBuf::from("/tmp/cortex mcp/cortex.db");
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
    let service_path = dir.path().join("cortex-ai-watch.service");
    let state_dir = dir.path().join("state");
    let db_path = dir.path().join("data/cortex.db");
    let user_home = dir.path().join("home");
    let bin = dir.path().join("bin/cortex");
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
    let script = debug_wrapper_script(std::path::Path::new("/home/me/workspace/cortex"));

    assert!(script.contains(r#"repo="${CORTEX_REPO:-/home/me/workspace/cortex}""#));
    assert!(script.contains(r#"repo="${HOME}/workspace/cortex""#));
    assert!(script.contains(r#"export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-.cache/cargo}""#));
    assert!(script.contains("CORTEX_DOCKER_INGEST_ENABLED"));
    assert!(script.contains("CORTEX_AUTH_MODE"));
    assert!(script.contains("cargo build --quiet --bin cortex"));
    assert!(script.contains(r#"exec "${CARGO_TARGET_DIR}/debug/cortex" "$@""#));
}

#[test]
fn debug_wrapper_content_phase_detects_stale_wrapper() {
    let dir = tempfile::tempdir().unwrap();
    let wrapper = dir.path().join("cortex");
    std::fs::write(&wrapper, "#!/bin/sh\nexec old\n").unwrap();

    let phase = check_debug_wrapper_content_phase(&wrapper, dir.path());

    assert!(matches!(phase.status, SetupStatus::Error));
    assert!(phase
        .detail
        .contains("does not match generated debug wrapper"));
}

#[test]
fn debug_compose_override_builds_local_debug_image_from_repo() {
    let override_yaml = debug_compose_override(std::path::Path::new("/home/me/workspace/cortex"));

    assert!(override_yaml.contains("image: cortex:local-debug"));
    assert!(override_yaml.contains("context: /home/me/workspace/cortex"));
    assert!(override_yaml.contains("dockerfile: config/Dockerfile"));
    assert!(override_yaml.contains("CORTEX_BUILD_PROFILE: debug"));
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
        "indexed files=3 ingested=2 duplicates=1 parse_errors=4 storage_blocked=0 dropped_metadata_fields=0 file_errors=1; inspect with `cortex ai errors --limit 20`, `cortex ai checkpoints --errors`, then rerun `cortex ai index --json` after fixes"
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
