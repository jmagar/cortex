use std::io::Write;

use crate::config::StorageConfig;
use crate::db::{SearchParams, init_pool, search_logs};
use serial_test::serial;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

#[test]
fn parses_zsh_extended_history_line() {
    let parsed = parse_zsh_extended_history_line(": 1716500000:3;cargo test").unwrap();

    assert_eq!(parsed.duration_secs, 3);
    assert_eq!(parsed.command, "cargo test");
    assert_eq!(parsed.started_at.timestamp(), 1_716_500_000);
}

#[test]
fn skips_plain_zsh_history_line_without_timestamp() {
    assert!(parse_zsh_extended_history_line("cargo test").is_none());
}

#[test]
fn command_scrubber_redacts_shell_specific_secret_forms() {
    let command = "env OPENAI_API_KEY=sk-proj-123 gh auth token --token abc curl -u user:pass https://user:pass@example.test";
    let scrubbed = scrub_command(command);

    assert!(!scrubbed.contains("sk-proj-123"));
    assert!(!scrubbed.contains("abc"));
    assert!(!scrubbed.contains("user:pass"));
    assert!(scrubbed.contains("[REDACTED]"));
}

#[test]
fn command_args_to_shell_command_quotes_multi_arg_invocations() {
    let args = vec![
        "sh".to_string(),
        "-lc".to_string(),
        "printf wrappedok >/dev/null".to_string(),
    ];

    assert_eq!(
        command_args_to_shell_command(&args),
        "sh -lc 'printf wrappedok >/dev/null'"
    );
    assert_eq!(
        command_args_to_shell_command(&["printf wrappedok >/dev/null".to_string()]),
        "printf wrappedok >/dev/null"
    );
}

#[test]
fn agent_command_ingest_spool_guard_is_argv_scoped() {
    // Canonical grammar: `cortex ingest shell agent index`.
    assert!(is_agent_command_ingest_spool_invocation(&[
        "cortex".to_string(),
        "ingest".to_string(),
        "shell".to_string(),
        "agent".to_string(),
        "index".to_string(),
    ]));
    assert!(is_agent_command_ingest_spool_invocation(&[
        "/usr/local/bin/cortex".to_string(),
        "ingest".to_string(),
        "shell".to_string(),
        "agent".to_string(),
        "index".to_string(),
        "--path".to_string(),
        "/tmp/x.jsonl".to_string(),
    ]));
    // Grouped grammar predating this rename: `cortex ingest agent-command
    // ingest-spool`. This is the one already deployed on live hosts (e.g.
    // dookie) and the only legacy shape worth tolerating here — the even
    // older bare `cortex agent-command ingest-spool` (no `ingest` prefix) is
    // unreachable: the CLI's top-level parser rejects it outright (see
    // `src/surfaces.rs`'s `MovedIntoGroupedDomain` entry), so no process can
    // ever actually invoke it for this guard to need to catch.
    assert!(is_agent_command_ingest_spool_invocation(&[
        "cortex".to_string(),
        "ingest".to_string(),
        "agent-command".to_string(),
        "ingest-spool".to_string(),
    ]));
    assert!(!is_agent_command_ingest_spool_invocation(&[
        "sh".to_string(),
        "-c".to_string(),
        "cortex ingest shell agent index".to_string(),
    ]));
    assert!(!is_agent_command_ingest_spool_invocation(&[
        "bash".to_string(),
        "-c".to_string(),
        "agent-command ingest-spool".to_string(),
    ]));
}

#[test]
fn sanitize_uri_segment_percent_encodes_losslessly() {
    assert_eq!(sanitize_uri_segment("a/b"), "a%2Fb");
    assert_eq!(sanitize_uri_segment("a b"), "a%20b");
    assert_eq!(sanitize_uri_segment("a-b"), "a-b");
    assert_eq!(sanitize_uri_segment("lambda-λ"), "lambda-%CE%BB");
}

#[cfg(unix)]
#[test]
#[serial]
fn wrapper_executes_multi_arg_commands_without_shell_reparse() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let fake_shell = dir.path().join("fake-shell");
    let arg_out = dir.path().join("args.txt");
    let spool_dir = dir.path().join("state");
    std::fs::create_dir(&spool_dir).unwrap();
    std::fs::set_permissions(&spool_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let spool = spool_dir.join("agent-command.jsonl");
    std::fs::write(
        &fake_shell,
        "#!/bin/sh\nprintf shell-used >\"$CORTEX_TEST_ARG_OUT\"\nexit 97\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake_shell, std::fs::Permissions::from_mode(0o755)).unwrap();
    let previous_shell = std::env::var_os("SHELL");
    let previous_out = std::env::var_os("CORTEX_TEST_ARG_OUT");
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("SHELL", &fake_shell) };
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("CORTEX_TEST_ARG_OUT", &arg_out) };

    let exit_code = run_agent_command_wrapper(
        &spool,
        &[
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf '%s\\n%s\\n%s\\n' \"$#\" \"$1\" \"$2\" >\"$CORTEX_TEST_ARG_OUT\"".to_string(),
            "sh".to_string(),
            "two words".to_string(),
            "literal;not-shell".to_string(),
        ],
    )
    .unwrap();

    match previous_shell {
        // TODO: Audit that the environment access only happens in single-threaded code.
        Some(value) => unsafe { std::env::set_var("SHELL", value) },
        // TODO: Audit that the environment access only happens in single-threaded code.
        None => unsafe { std::env::remove_var("SHELL") },
    }
    match previous_out {
        // TODO: Audit that the environment access only happens in single-threaded code.
        Some(value) => unsafe { std::env::set_var("CORTEX_TEST_ARG_OUT", value) },
        // TODO: Audit that the environment access only happens in single-threaded code.
        None => unsafe { std::env::remove_var("CORTEX_TEST_ARG_OUT") },
    }
    assert_eq!(exit_code, 0);
    assert_eq!(
        std::fs::read_to_string(arg_out).unwrap(),
        "2\ntwo words\nliteral;not-shell\n"
    );
}

#[test]
fn imports_zsh_history_as_shell_history_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cortex.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    let history = dir.path().join(".zsh_history");
    std::fs::write(
        &history,
        ": 1716500000:3;export API_KEY=abc123\nplain command\n",
    )
    .unwrap();

    let result = import_zsh_history(&pool, &history, "zsh").unwrap();

    assert_eq!(result.scanned, 2);
    assert_eq!(result.imported, 1);
    assert_eq!(result.skipped, 1);
    let rows = search_logs(
        &pool,
        &SearchParams {
            query: Some("export".into()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].facility.as_deref(), Some("shell"));
    assert_eq!(rows[0].app_name.as_deref(), Some("zsh"));
    assert!(
        rows[0]
            .metadata_json
            .as_deref()
            .unwrap()
            .contains("shell-history")
    );
    assert!(
        rows[0]
            .metadata_json
            .as_deref()
            .unwrap()
            .contains("\"shell\"")
    );
    assert!(rows[0].message.contains("[REDACTED]"));
}

#[test]
fn imports_zsh_history_from_saved_offset() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cortex.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    let history = dir.path().join(".zsh_history");
    let state = dir.path().join("shell-state.json");
    std::fs::write(&history, ": 1716500000:3;cargo test\n").unwrap();

    let first = import_zsh_history_with_state(&pool, &history, "zsh", &state).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&history)
        .unwrap()
        .write_all(b": 1716500010:1;cargo fmt\n")
        .unwrap();
    let second = import_zsh_history_with_state(&pool, &history, "zsh", &state).unwrap();
    let third = import_zsh_history_with_state(&pool, &history, "zsh", &state).unwrap();

    assert_eq!(first.scanned, 1);
    assert_eq!(first.imported, 1);
    assert_eq!(second.scanned, 1);
    assert_eq!(second.imported, 1);
    assert_eq!(third.scanned, 0);
    assert_eq!(third.imported, 0);
    let rows = search_logs(
        &pool,
        &SearchParams {
            query: Some("cargo".into()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn imports_atuin_history_as_shell_history_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cortex.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    let atuin = dir.path().join("history.db");
    let conn = rusqlite::Connection::open(&atuin).unwrap();
    conn.execute_batch(
        "CREATE TABLE history (
            id TEXT PRIMARY KEY,
            timestamp INTEGER NOT NULL,
            duration INTEGER NOT NULL,
            exit INTEGER NOT NULL,
            command TEXT NOT NULL,
            cwd TEXT NOT NULL,
            session TEXT NOT NULL,
            hostname TEXT NOT NULL,
            deleted_at INTEGER,
            author TEXT,
            intent TEXT
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO history (id, timestamp, duration, exit, command, cwd, session, hostname)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            "hist-1",
            1_716_500_000_123_000_000_i64,
            3_000_000_000_i64,
            2_i64,
            "export API_KEY=abc123",
            "/tmp/project",
            "session-1",
            "dookie"
        ],
    )
    .unwrap();

    let result = import_atuin_history_with_state(
        &pool,
        &atuin,
        dir.path().join("atuin-state.json").as_path(),
    )
    .unwrap();

    assert_eq!(result.scanned, 1);
    assert_eq!(result.imported, 1);
    let rows = search_logs(
        &pool,
        &SearchParams {
            query: Some("export".into()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].hostname, "dookie");
    assert_eq!(rows[0].facility.as_deref(), Some("shell"));
    assert_eq!(rows[0].app_name.as_deref(), Some("atuin"));
    assert_eq!(rows[0].severity, "warning");
    assert!(rows[0].message.contains("[REDACTED]"));
    let metadata = rows[0].metadata_json.as_deref().unwrap();
    assert!(metadata.contains("\"source_kind\":\"shell-history\""));
    assert!(metadata.contains("\"session\":\"session-1\""));
    assert!(metadata.contains("\"cwd\":\"/tmp/project\""));
}

#[test]
fn imports_atuin_history_from_saved_timestamp_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cortex.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    let atuin = dir.path().join("history.db");
    let state = dir.path().join("atuin-state.json");
    let conn = rusqlite::Connection::open(&atuin).unwrap();
    conn.execute_batch(
        "CREATE TABLE history (
            id TEXT PRIMARY KEY,
            timestamp INTEGER NOT NULL,
            duration INTEGER NOT NULL,
            exit INTEGER NOT NULL,
            command TEXT NOT NULL,
            cwd TEXT NOT NULL,
            session TEXT NOT NULL,
            hostname TEXT NOT NULL,
            deleted_at INTEGER,
            author TEXT,
            intent TEXT
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO history (id, timestamp, duration, exit, command, cwd, session, hostname)
         VALUES ('hist-1', 1716500000000000000, 1000, 0, 'cargo test', '/tmp/project', 's1', 'dookie')",
        [],
    )
    .unwrap();

    let first = import_atuin_history_with_state(&pool, &atuin, &state).unwrap();
    conn.execute(
        "INSERT INTO history (id, timestamp, duration, exit, command, cwd, session, hostname)
         VALUES ('hist-2', 1716500001000000000, 1000, 0, 'cargo fmt', '/tmp/project', 's1', 'dookie')",
        [],
    )
    .unwrap();
    let second = import_atuin_history_with_state(&pool, &atuin, &state).unwrap();
    let third = import_atuin_history_with_state(&pool, &atuin, &state).unwrap();

    assert_eq!(first.scanned, 1);
    assert_eq!(first.imported, 1);
    assert_eq!(second.scanned, 1);
    assert_eq!(second.imported, 1);
    assert_eq!(third.scanned, 0);
    assert_eq!(third.imported, 0);
}

#[test]
fn imports_agent_spool_as_agent_command_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cortex.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    let spool_dir = dir.path().join("private-state");
    std::fs::create_dir(&spool_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&spool_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let spool = spool_dir.join("claude-commands.jsonl");
    let auth_header = format!("{} {}", "Authorization:", "Bearer test-token");
    let record = AgentCommandSpoolRecord {
        started_at: "2026-05-24T05:00:00.000Z".into(),
        finished_at: "2026-05-24T05:00:01.000Z".into(),
        duration_ms: 1000,
        exit_status: Some(2),
        command: format!("curl -H '{auth_header}' http://example.test"),
        cwd: Some("/tmp/project".into()),
        agent: "claude-code".into(),
        command_surface: Some("bash_tool".into()),
        hostname: "dookie".into(),
        user: Some("jmagar".into()),
        pid: 42,
        session_id: Some("session-1".into()),
        schema_version: 1,
        content_scrubbed: false,
    };
    let mut file = std::fs::File::create(&spool).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&spool, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    serde_json::to_writer(&mut file, &record).unwrap();
    writeln!(file).unwrap();

    let result = import_agent_command_spool(&pool, &spool).unwrap();

    assert_eq!(result.imported, 1);
    let rows = search_logs(
        &pool,
        &SearchParams {
            query: Some("curl".into()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].facility.as_deref(), Some("agent"));
    assert_eq!(rows[0].severity, "warning");
    assert_eq!(rows[0].ai_tool.as_deref(), Some("claude-code"));
    assert!(rows[0].message.contains("[REDACTED]"));
    assert!(
        rows[0]
            .metadata_json
            .as_deref()
            .unwrap()
            .contains("agent-command")
    );
    assert!(
        rows[0]
            .metadata_json
            .as_deref()
            .unwrap()
            .contains("agent_command")
    );
    assert_eq!(std::fs::read_to_string(&spool).unwrap(), "");
    let second = import_agent_command_spool(&pool, &spool).unwrap();
    assert_eq!(second.scanned, 0);
    assert_eq!(second.imported, 0);
}

fn sample_agent_command_record(command: &str) -> AgentCommandSpoolRecord {
    AgentCommandSpoolRecord {
        started_at: "2026-07-06T00:00:00Z".to_string(),
        finished_at: "2026-07-06T00:00:01Z".to_string(),
        duration_ms: 1000,
        exit_status: Some(0),
        command: command.to_string(),
        cwd: None,
        agent: "claude-code".to_string(),
        command_surface: None,
        hostname: "testhost".to_string(),
        user: None,
        pid: 1234,
        session_id: None,
        schema_version: 1,
        content_scrubbed: true,
    }
}

#[test]
fn import_agent_command_records_dedupes_against_existing_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cortex.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    let record = sample_agent_command_record("echo hi");

    let first = import_agent_command_records(&pool, std::slice::from_ref(&record), None).unwrap();
    assert_eq!(first.imported, 1);
    assert_eq!(first.skipped_duplicates, 0);

    let second = import_agent_command_records(&pool, &[record], None).unwrap();
    assert_eq!(second.imported, 0);
    assert_eq!(second.skipped_duplicates, 1);
}

#[test]
fn import_agent_command_records_annotates_forwarded_peer_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cortex.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    let record = sample_agent_command_record("echo hi");

    let result = import_agent_command_records(&pool, &[record], Some("203.0.113.7")).unwrap();
    assert_eq!(result.imported, 1);

    // Query the inserted row directly to prove the peer IP actually landed
    // in metadata_json, rather than just asserting the call didn't panic.
    // Acquire and drop a pool connection per query rather than holding one
    // across the second `import_agent_command_records` call below, which
    // also needs a connection from the same (small, test-sized) pool.
    let metadata_json: String = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT metadata_json FROM logs WHERE message = ?1",
            ["echo hi"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        metadata_json.contains("203.0.113.7"),
        "expected forwarded_from_peer_ip in metadata_json, got: {metadata_json}"
    );

    // A second record with no forwarding peer must NOT gain the field.
    let local_record = sample_agent_command_record("echo local");
    import_agent_command_records(&pool, &[local_record], None).unwrap();
    let local_metadata_json: String = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT metadata_json FROM logs WHERE message = ?1",
            ["echo local"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!local_metadata_json.contains("forwarded_from_peer_ip"));
}

#[tokio::test]
async fn forward_agent_command_spool_posts_and_truncates_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let spool_path = dir.path().join("agent-command.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&spool_path)
        .unwrap();
    writeln!(
        file,
        r#"{{"started_at":"2026-07-06T00:00:00Z","finished_at":"2026-07-06T00:00:01Z","duration_ms":1000,"exit_status":0,"command":"echo hi","cwd":null,"agent":"claude-code","command_surface":null,"hostname":"testhost","user":null,"pid":1234,"session_id":null,"schema_version":1,"content_scrubbed":true}}"#
    )
    .unwrap();
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&spool_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agent-commands"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"scanned":0,"imported":1,"skipped":0,"skipped_duplicates":0,"errors":0}"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let result = forward_agent_command_spool(&spool_path, &server.uri(), Some("secret"))
        .await
        .unwrap();

    assert_eq!(result.imported, 1);
    let remaining = std::fs::metadata(&spool_path).unwrap();
    assert_eq!(
        remaining.len(),
        0,
        "spool must be truncated after a successful forward"
    );
}

#[tokio::test]
async fn forward_agent_command_spool_keeps_file_on_http_failure() {
    let dir = tempfile::tempdir().unwrap();
    let spool_path = dir.path().join("agent-command.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&spool_path)
        .unwrap();
    writeln!(
        file,
        r#"{{"started_at":"2026-07-06T00:00:00Z","finished_at":"2026-07-06T00:00:01Z","duration_ms":1000,"exit_status":0,"command":"echo hi","cwd":null,"agent":"claude-code","command_surface":null,"hostname":"testhost","user":null,"pid":1234,"session_id":null,"schema_version":1,"content_scrubbed":true}}"#
    )
    .unwrap();
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&spool_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agent-commands"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;

    let error = forward_agent_command_spool(&spool_path, &server.uri(), None)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("503"), "got: {error}");
    let remaining = std::fs::metadata(&spool_path).unwrap();
    assert!(remaining.len() > 0, "spool must survive a failed forward");
}

#[test]
fn agent_spool_malformed_line_with_multibyte_at_preview_boundary_no_panic() {
    // Regression (bead syslog-mcp-8ouq): the JSON-parse error branch logs an
    // 80-byte preview of the offending line. The original code byte-sliced at
    // index 80, panicking when a multi-byte UTF-8 character straddled that
    // boundary. Sweep pad lengths so the 4-byte emoji covers every alignment
    // around byte 80, including mid-character offsets.
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cortex.db");
    let pool = init_pool(&StorageConfig::for_test(db_path)).unwrap();
    let spool_dir = dir.path().join("private-state");
    std::fs::create_dir(&spool_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&spool_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let spool = spool_dir.join("claude-commands.jsonl");
    let mut file = std::fs::File::create(&spool).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&spool, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut expected_errors = 0;
    for pad in 60..=80 {
        // Not valid JSON, longer than 80 bytes, multibyte char near byte 80.
        writeln!(file, "{}\u{1F600}tail-not-json", "a".repeat(pad)).unwrap();
        expected_errors += 1;
    }
    drop(file);

    let result = import_agent_command_spool(&pool, &spool).unwrap();

    assert_eq!(result.errors, expected_errors);
    assert_eq!(result.imported, 0);
}

#[test]
#[serial]
fn wrapper_preserves_command_exit_when_spool_append_fails() {
    // `["true"]` is a single token, so the wrapper runs it via `$SHELL -c true`
    // (see `command_status`). This must be `#[serial]` to exclude
    // `wrapper_executes_multi_arg_commands_without_shell_reparse`, which mutates
    // the global `SHELL`/`CORTEX_TEST_ARG_OUT` env — overlapping would exec that
    // test's fake shell here and corrupt its output buffer (both tests fail).
    let dir = tempfile::tempdir().unwrap();

    let exit_code =
        run_agent_command_wrapper(dir.path(), &["true".to_string()]).expect("wrapper runs command");

    assert_eq!(exit_code, 0);
}

#[cfg(unix)]
#[test]
fn existing_spool_parent_permissions_are_not_mutated() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("existing-parent");
    std::fs::create_dir(&parent).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();

    ensure_private_parent(&parent.join("agent-command.jsonl")).unwrap();

    let mode = std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
}

#[cfg(unix)]
#[test]
fn newly_created_spool_parent_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("new-parent");

    ensure_private_parent(&parent.join("agent-command.jsonl")).unwrap();

    let mode = std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}
