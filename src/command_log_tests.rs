use std::io::Write;

use crate::config::StorageConfig;
use crate::db::{init_pool, search_logs, SearchParams};

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
