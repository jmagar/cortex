use std::io::Write;

use crate::config::StorageConfig;
use crate::db::{init_pool, search_logs, SearchParams};
use serial_test::serial;

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
    assert!(is_agent_command_ingest_spool_invocation(&[
        "/usr/local/bin/syslog".to_string(),
        "agent-command".to_string(),
        "ingest-spool".to_string(),
        "--path".to_string(),
        "/tmp/spool.jsonl".to_string(),
    ]));
    assert!(!is_agent_command_ingest_spool_invocation(&[
        "echo".to_string(),
        "agent-command ingest-spool".to_string(),
    ]));
    assert!(!is_agent_command_ingest_spool_invocation(&[
        "syslog".to_string(),
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
        "#!/bin/sh\nprintf shell-used >\"$SYSLOG_TEST_ARG_OUT\"\nexit 97\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake_shell, std::fs::Permissions::from_mode(0o755)).unwrap();
    let previous_shell = std::env::var_os("SHELL");
    let previous_out = std::env::var_os("SYSLOG_TEST_ARG_OUT");
    std::env::set_var("SHELL", &fake_shell);
    std::env::set_var("SYSLOG_TEST_ARG_OUT", &arg_out);

    let exit_code = run_agent_command_wrapper(
        &spool,
        &[
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf '%s\\n%s\\n%s\\n' \"$#\" \"$1\" \"$2\" >\"$SYSLOG_TEST_ARG_OUT\"".to_string(),
            "sh".to_string(),
            "two words".to_string(),
            "literal;not-shell".to_string(),
        ],
    )
    .unwrap();

    match previous_shell {
        Some(value) => std::env::set_var("SHELL", value),
        None => std::env::remove_var("SHELL"),
    }
    match previous_out {
        Some(value) => std::env::set_var("SYSLOG_TEST_ARG_OUT", value),
        None => std::env::remove_var("SYSLOG_TEST_ARG_OUT"),
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
    let db_path = dir.path().join("syslog.db");
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
    assert!(rows[0]
        .metadata_json
        .as_deref()
        .unwrap()
        .contains("shell-history"));
    assert!(rows[0]
        .metadata_json
        .as_deref()
        .unwrap()
        .contains("\"shell\""));
    assert!(rows[0].message.contains("[REDACTED]"));
}

#[test]
fn imports_zsh_history_from_saved_offset() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("syslog.db");
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
    let db_path = dir.path().join("syslog.db");
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
    let db_path = dir.path().join("syslog.db");
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
    let db_path = dir.path().join("syslog.db");
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
    assert!(rows[0]
        .metadata_json
        .as_deref()
        .unwrap()
        .contains("agent-command"));
    assert!(rows[0]
        .metadata_json
        .as_deref()
        .unwrap()
        .contains("agent_command"));
    assert_eq!(std::fs::read_to_string(&spool).unwrap(), "");
    let second = import_agent_command_spool(&pool, &spool).unwrap();
    assert_eq!(second.scanned, 0);
    assert_eq!(second.imported, 0);
}

#[test]
fn wrapper_preserves_command_exit_when_spool_append_fails() {
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
