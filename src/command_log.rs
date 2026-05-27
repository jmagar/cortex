use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::LazyLock;
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::db::{self, LogBatchEntry};
use crate::enrich::{stamp_source_kind, SourceKind};
use crate::ingest_metadata::bounded_metadata_json;
use crate::syslog::enrichment::scrub_ai_message;

static COMMAND_SECRET_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r#"(?i)\bAuthorization:\s*Bearer\s+['"]?[^'"\s]+"#,
        r#"(?i)(?:^|\s)(?:--?(?:api[-_]?key|token|secret|password|passwd|client[-_]?secret))=("[^"]*"|'[^']*'|[^\s]+)"#,
        r#"(?i)(?:^|\s)(?:--?(?:api[-_]?key|token|secret|password|passwd|client[-_]?secret))\s+("[^"]*"|'[^']*'|[^\s]+)"#,
        r#"(?i)\b(?:[A-Z_][A-Z0-9_]*(?:TOKEN|KEY|SECRET|PASSWORD|PASS|PWD|AUTH)[A-Z0-9_]*)=("[^"]*"|'[^']*'|[^\s]+)"#,
        r#"(?i)\bcurl\s+-u\s+("[^"]*"|'[^']*'|[^\s]+)"#,
        r#"(?i)\bhttps?://[^/\s:@]+:[^/\s@]+@"#,
        r#"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----"#,
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("static command secret regex"))
    .collect()
});

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandLogImportResult {
    pub scanned: usize,
    pub imported: usize,
    pub skipped: usize,
    pub skipped_duplicates: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZshHistoryRecord {
    pub started_at: DateTime<Utc>,
    pub duration_secs: u64,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentCommandSpoolRecord {
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    pub exit_status: Option<i32>,
    pub command: String,
    pub cwd: Option<String>,
    pub agent: String,
    pub command_surface: Option<String>,
    pub hostname: String,
    pub user: Option<String>,
    pub pid: u32,
    pub session_id: Option<String>,
    #[serde(default = "schema_version_one")]
    pub schema_version: u32,
    #[serde(default)]
    pub content_scrubbed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct ShellHistoryImportState {
    path: String,
    shell: String,
    offset: u64,
    #[serde(default)]
    line_no: usize,
}

pub fn import_zsh_history(
    pool: &db::DbPool,
    path: &Path,
    shell: &str,
) -> Result<CommandLogImportResult> {
    let state_path = shell_history_state_path(path, shell)?;
    import_zsh_history_with_state(pool, path, shell, &state_path)
}

fn import_zsh_history_with_state(
    pool: &db::DbPool,
    path: &Path,
    shell: &str,
    state_path: &Path,
) -> Result<CommandLogImportResult> {
    let mut file =
        fs::File::open(path).with_context(|| format!("open shell history {}", path.display()))?;
    let file_len = file
        .metadata()
        .with_context(|| format!("stat shell history {}", path.display()))?
        .len();
    let mut state = read_shell_history_state(state_path, path, shell)?.unwrap_or_default();
    if state.offset > file_len {
        state.offset = 0;
        state.line_no = 0;
    }
    file.seek(SeekFrom::Start(state.offset))
        .with_context(|| format!("seek shell history {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let hostname = hostname();
    let user = username();
    let mut result = CommandLogImportResult::default();
    let mut batch = Vec::new();
    let mut line = String::new();
    let mut line_no = state.line_no;

    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .with_context(|| format!("read shell history {}", path.display()))?;
        if bytes == 0 {
            break;
        }
        result.scanned += 1;
        line_no += 1;
        let line = line.trim_end_matches(['\r', '\n']);
        let Some(record) = parse_zsh_extended_history_line(line) else {
            result.skipped += 1;
            continue;
        };
        let entry = zsh_record_to_entry(&record, path, line_no, shell, &hostname, user.as_deref());
        if entry_exists(pool, &entry)? {
            result.skipped_duplicates += 1;
        } else {
            batch.push(entry);
        }
    }

    let next_offset = reader
        .stream_position()
        .with_context(|| format!("tell shell history {}", path.display()))?;
    if !batch.is_empty() {
        result.imported = db::insert_logs_batch(pool, &batch)?;
    }
    write_shell_history_state(state_path, path, shell, next_offset, line_no)?;
    Ok(result)
}

pub fn import_agent_command_spool(
    pool: &db::DbPool,
    path: &Path,
) -> Result<CommandLogImportResult> {
    validate_spool_path_for_read(path)?;
    let mut file = open_spool_for_update(path)?;
    lock_file_exclusive(&file, path)?;
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("seek agent command spool {}", path.display()))?;
    let reader = BufReader::new(&mut file);
    let mut result = CommandLogImportResult::default();
    let mut batch = Vec::new();

    for line in reader.lines() {
        result.scanned += 1;
        let line = match line {
            Ok(line) => line,
            Err(_) => {
                result.errors += 1;
                continue;
            }
        };
        if line.trim().is_empty() {
            result.skipped += 1;
            continue;
        }
        match serde_json::from_str::<AgentCommandSpoolRecord>(&line) {
            Ok(record) => {
                let entry = agent_record_to_entry(&record);
                if entry_exists(pool, &entry)? {
                    result.skipped_duplicates += 1;
                } else {
                    batch.push(entry);
                }
            }
            Err(_) => result.errors += 1,
        }
    }

    if !batch.is_empty() {
        result.imported = db::insert_logs_batch(pool, &batch)?;
    }
    file.set_len(0)
        .with_context(|| format!("truncate agent command spool {}", path.display()))?;
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("rewind agent command spool {}", path.display()))?;
    Ok(result)
}

pub fn run_agent_command_wrapper(spool_path: &Path, command_args: &[String]) -> Result<i32> {
    anyhow::ensure!(
        !command_args.is_empty(),
        "agent-command wrap requires COMMAND after --"
    );
    ensure_private_parent(spool_path)?;

    let command = command_args_to_shell_command(command_args);
    if should_run_agent_command_unwrapped(command_args) {
        return run_command_unwrapped(command_args, &command);
    }
    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string());
    let started = Utc::now();
    let timer = Instant::now();
    let status = command_status(command_args, &command).context("run wrapped command")?;
    let finished = Utc::now();
    let duration_ms = timer.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let record = AgentCommandSpoolRecord {
        started_at: rfc3339(started),
        finished_at: rfc3339(finished),
        duration_ms,
        exit_status: status.code(),
        command: scrub_command(&command),
        cwd,
        agent: std::env::var("SYSLOG_AGENT_COMMAND_AGENT")
            .unwrap_or_else(|_| "claude-code".to_string()),
        command_surface: std::env::var("SYSLOG_AGENT_COMMAND_SURFACE").ok(),
        hostname: hostname(),
        user: username(),
        pid: std::process::id(),
        session_id: std::env::var("CLAUDE_CODE_SESSION_ID")
            .ok()
            .or_else(|| std::env::var("CLAUDE_SESSION_ID").ok())
            .or_else(|| std::env::var("SYSLOG_AGENT_COMMAND_SESSION").ok()),
        schema_version: 1,
        content_scrubbed: true,
    };
    if let Err(error) = append_spool_record(spool_path, &record) {
        eprintln!("syslog agent-command: failed to append command record: {error:#}");
    }
    Ok(status.code().unwrap_or(1))
}

pub fn scrub_command(command: &str) -> String {
    let mut out = scrub_ai_message(command, None);
    for pattern in COMMAND_SECRET_PATTERNS.iter() {
        out = pattern.replace_all(&out, "[REDACTED]").into_owned();
    }
    out
}

fn run_command_unwrapped(command_args: &[String], fallback_shell_command: &str) -> Result<i32> {
    Ok(command_status(command_args, fallback_shell_command)?
        .code()
        .unwrap_or(1))
}

fn command_args_to_shell_command(command_args: &[String]) -> String {
    if command_args.len() == 1 {
        command_args[0].clone()
    } else {
        command_args
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn should_run_agent_command_unwrapped(command_args: &[String]) -> bool {
    std::env::var("SYSLOG_AGENT_COMMAND_WRAPPER")
        .ok()
        .as_deref()
        == Some("1")
        || is_agent_command_ingest_spool_invocation(command_args)
}

fn is_agent_command_ingest_spool_invocation(command_args: &[String]) -> bool {
    let Some(program) = command_args.first() else {
        return false;
    };
    let program_name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program);
    program_name == "syslog"
        && command_args.get(1).map(String::as_str) == Some("agent-command")
        && command_args.get(2).map(String::as_str) == Some("ingest-spool")
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':' | b'=' | b',' | b'+')
    }) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn command_status(command_args: &[String], fallback_shell_command: &str) -> Result<ExitStatus> {
    if command_args.len() == 1 {
        return shell_command_status(fallback_shell_command);
    }
    let (program, args) = command_args
        .split_first()
        .expect("wrapper validates command args are not empty");
    Command::new(program)
        .args(args)
        .env("SYSLOG_AGENT_COMMAND_WRAPPER", "1")
        .env_remove("CLAUDE_CODE_SHELL_PREFIX")
        .status()
        .with_context(|| format!("run command {}", shell_quote(program)))
}

fn shell_command_status(command: &str) -> Result<ExitStatus> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    Command::new(shell)
        .arg("-lc")
        .arg(command)
        .env("SYSLOG_AGENT_COMMAND_WRAPPER", "1")
        .env_remove("CLAUDE_CODE_SHELL_PREFIX")
        .status()
        .context("run command")
}

pub fn parse_zsh_extended_history_line(line: &str) -> Option<ZshHistoryRecord> {
    let rest = line.strip_prefix(": ")?;
    let (epoch, rest) = rest.split_once(':')?;
    let (duration, command) = rest.split_once(';')?;
    let epoch = epoch.trim().parse::<i64>().ok()?;
    let duration_secs = duration.trim().parse::<u64>().ok()?;
    let started_at = Utc.timestamp_opt(epoch, 0).single()?;
    Some(ZshHistoryRecord {
        started_at,
        duration_secs,
        command: command.to_string(),
    })
}

fn zsh_record_to_entry(
    record: &ZshHistoryRecord,
    path: &Path,
    line_no: usize,
    shell: &str,
    hostname: &str,
    user: Option<&str>,
) -> LogBatchEntry {
    let command = scrub_command(&record.command);
    let metadata_json = bounded_metadata_json(serde_json::json!({
        "source_type": "shell_history",
        "source_kind": SourceKind::ShellHistory.as_str(),
        "shell": {
            "name": shell,
            "user": user,
            "history_path": path.display().to_string(),
            "line_no": line_no,
            "duration_secs": record.duration_secs,
            "timestamp_quality": "zsh_extended_history"
        },
        "content_scrubbed": true,
    }));
    let mut entry = LogBatchEntry {
        timestamp: rfc3339(record.started_at),
        hostname: hostname.to_string(),
        facility: Some("shell".to_string()),
        severity: "info".to_string(),
        app_name: Some(shell.to_string()),
        process_id: None,
        message: command.clone(),
        raw: command,
        source_ip: format!(
            "shell-history://{}/{}/{}",
            sanitize_uri_segment(hostname),
            sanitize_uri_segment(user.unwrap_or("unknown")),
            sanitize_uri_segment(shell)
        ),
        docker_checkpoint: None,
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
        metadata_json: Some(metadata_json),
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    };
    stamp_source_kind(&mut entry, SourceKind::ShellHistory);
    entry
}

fn agent_record_to_entry(record: &AgentCommandSpoolRecord) -> LogBatchEntry {
    let command = scrub_command(&record.command);
    let exit_status = record.exit_status;
    let severity = match exit_status {
        Some(0) => "info",
        Some(_) => "warning",
        None => "err",
    };
    let metadata_json = bounded_metadata_json(serde_json::json!({
        "source_type": "agent_command",
        "source_kind": SourceKind::AgentCommand.as_str(),
        "agent_command": {
            "schema_version": record.schema_version,
            "agent": record.agent,
            "command_surface": record.command_surface,
            "cwd": record.cwd,
            "user": record.user,
            "pid": record.pid,
            "exit_status": exit_status,
            "duration_ms": record.duration_ms,
            "finished_at": record.finished_at,
            "session_id": record.session_id
        },
        "content_scrubbed": true,
    }));
    let mut entry = LogBatchEntry {
        timestamp: normalize_or_now(&record.started_at),
        hostname: record.hostname.clone(),
        facility: Some("agent".to_string()),
        severity: severity.to_string(),
        app_name: Some(record.agent.clone()),
        process_id: Some(record.pid.to_string()),
        message: command.clone(),
        raw: command,
        source_ip: format!(
            "agent-command://{}/{}/{}",
            sanitize_uri_segment(&record.hostname),
            sanitize_uri_segment(&record.agent),
            sanitize_uri_segment(record.session_id.as_deref().unwrap_or("unknown"))
        ),
        docker_checkpoint: None,
        ai_tool: Some(record.agent.clone()),
        ai_project: record.cwd.clone(),
        ai_session_id: record.session_id.clone(),
        ai_transcript_path: None,
        metadata_json: Some(metadata_json),
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: Some("command".to_string()),
        parse_error: None,
    };
    stamp_source_kind(&mut entry, SourceKind::AgentCommand);
    entry
}

fn append_spool_record(path: &Path, record: &AgentCommandSpoolRecord) -> Result<()> {
    validate_spool_path_for_write(path)?;
    let mut file = open_spool_for_append(path)?;
    lock_file_exclusive(&file, path)?;
    serde_json::to_writer(&mut file, record)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn open_spool_for_update(path: &Path) -> Result<fs::File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .with_context(|| format!("open agent command spool {}", path.display()))
}

fn open_spool_for_append(path: &Path) -> Result<fs::File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .with_context(|| format!("open agent command spool {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod agent command spool {}", path.display()))?;
    }
    Ok(file)
}

fn lock_file_exclusive(file: &fs::File, path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        anyhow::ensure!(
            rc == 0,
            "lock file {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
    }
    #[cfg(not(unix))]
    {
        let _ = (file, path);
    }
    Ok(())
}

fn shell_history_state_path(path: &Path, shell: &str) -> Result<PathBuf> {
    let state_dir = command_log_state_dir(path)?;
    let identity = format!("{}\0{}", shell, path.display());
    Ok(state_dir.join(format!(
        "shell-history-{:016x}.json",
        stable_hash(&identity)
    )))
}

fn command_log_state_dir(path: &Path) -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("SYSLOG_COMMAND_LOG_STATE_DIR") {
        let state_dir = PathBuf::from(value);
        ensure_private_parent(&state_dir.join(".keep"))?;
        return Ok(state_dir);
    }
    if let Some(value) = std::env::var_os("XDG_STATE_HOME") {
        let state_dir = PathBuf::from(value).join("syslog-mcp");
        ensure_private_parent(&state_dir.join(".keep"))?;
        return Ok(state_dir);
    }
    if let Some(value) = std::env::var_os("HOME") {
        let state_dir = PathBuf::from(value).join(".local/state/syslog-mcp");
        ensure_private_parent(&state_dir.join(".keep"))?;
        return Ok(state_dir);
    }
    let fallback = path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".syslog-mcp-state");
    ensure_private_parent(&fallback.join(".keep"))?;
    Ok(fallback)
}

fn read_shell_history_state(
    state_path: &Path,
    history_path: &Path,
    shell: &str,
) -> Result<Option<ShellHistoryImportState>> {
    let raw = match fs::read_to_string(state_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read shell history state {}", state_path.display()))
        }
    };
    let state: ShellHistoryImportState = serde_json::from_str(&raw)
        .with_context(|| format!("parse shell history state {}", state_path.display()))?;
    if state.path == history_path.display().to_string() && state.shell == shell {
        Ok(Some(state))
    } else {
        Ok(None)
    }
}

fn write_shell_history_state(
    state_path: &Path,
    history_path: &Path,
    shell: &str,
    offset: u64,
    line_no: usize,
) -> Result<()> {
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create shell history state dir {}", parent.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("chmod shell history state dir {}", parent.display()))?;
        }
    }
    let state = ShellHistoryImportState {
        path: history_path.display().to_string(),
        shell: shell.to_string(),
        offset,
        line_no,
    };
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(state_path)
        .with_context(|| format!("open shell history state {}", state_path.display()))?;
    serde_json::to_writer(&mut file, &state)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn entry_exists(pool: &db::DbPool, entry: &LogBatchEntry) -> Result<bool> {
    let conn = pool.get()?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM logs WHERE source_ip = ?1 AND timestamp = ?2 AND message = ?3",
        (&entry.source_ip, &entry.timestamp, &entry.message),
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn ensure_private_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let parent_existed = parent.exists();
    fs::create_dir_all(&parent)
        .with_context(|| format!("create spool parent {}", parent.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !parent_existed {
            fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("chmod spool parent {}", parent.display()))?;
        }
    }
    Ok(())
}

fn validate_spool_path_for_write(path: &Path) -> Result<()> {
    reject_symlink(path)?;
    if let Some(parent) = path.parent() {
        reject_unsafe_parent(parent)?;
    }
    Ok(())
}

fn validate_spool_path_for_read(path: &Path) -> Result<()> {
    reject_symlink(path)?;
    if let Some(parent) = path.parent() {
        reject_unsafe_parent(parent)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mode = fs::metadata(path)
            .with_context(|| format!("stat spool {}", path.display()))?
            .mode()
            & 0o777;
        anyhow::ensure!(
            mode & 0o077 == 0,
            "agent command spool must not be readable or writable by group/other: {}",
            path.display()
        );
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "agent command spool must not be a symlink: {}",
            path.display()
        );
    }
    Ok(())
}

fn reject_unsafe_parent(parent: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if parent.exists() {
            let mode = fs::metadata(parent)
                .with_context(|| format!("stat spool parent {}", parent.display()))?
                .mode()
                & 0o777;
            anyhow::ensure!(
                mode & 0o022 == 0,
                "agent command spool parent must not be group/world writable: {}",
                parent.display()
            );
        }
    }
    Ok(())
}

fn schema_version_one() -> u32 {
    1
}

fn normalize_or_now(value: &str) -> String {
    match DateTime::parse_from_rfc3339(value) {
        Ok(dt) => rfc3339(dt.with_timezone(&Utc)),
        Err(e) => {
            tracing::warn!(
                raw_timestamp = value,
                error = %e,
                "agent command spool record has unparseable started_at; substituting import time"
            );
            rfc3339(Utc::now())
        }
    }
}

fn rfc3339(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

fn username() -> Option<String> {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("LOGNAME").ok())
        .filter(|value| !value.trim().is_empty())
}

fn sanitize_uri_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(*byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
#[path = "command_log_tests.rs"]
mod tests;
