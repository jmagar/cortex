use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::config::StorageConfig;
use crate::db::{enforce_storage_budget, insert_logs_batch_in_tx, DbPool, LogBatchEntry};
use crate::syslog::enrichment::{project_from_transcript_path, scrub_ai_message};

mod checkpoint;
mod claude;
mod codex;

pub use checkpoint::CheckpointStore;

const MAX_FILE_SIZE_BYTES: u64 = 1024 * 1024 * 1024;
#[cfg(not(test))]
const MAX_RECORD_SIZE_BYTES: usize = 512 * 1024 * 1024;
#[cfg(test)]
const MAX_RECORD_SIZE_BYTES: usize = 16 * 1024 * 1024;
const MAX_INDEX_CHUNK_RECORDS: usize = 500;
const MAX_INDEX_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const MAX_AI_PROJECT_CHARS: usize = 512;
const MAX_AI_SESSION_ID_CHARS: usize = 128;
const MAX_TRANSCRIPT_PATH_CHARS: usize = 1024;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct IndexResult {
    pub discovered_files: usize,
    pub ingested: usize,
    pub skipped_dupes: usize,
    pub parse_errors: usize,
    pub skipped_files: usize,
    pub unsupported_files: usize,
    pub skipped_symlinks: usize,
    pub skipped_unsafe_paths: usize,
    pub storage_blocked_chunks: usize,
    pub dropped_metadata_fields: usize,
    pub checkpoint_updates: usize,
    pub file_errors: Vec<IndexFileError>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IndexFileError {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    ClaudeProject,
    CodexSession,
    ExplicitFile,
}

impl SourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeProject => "claude_project",
            Self::CodexSession => "codex_session",
            Self::ExplicitFile => "explicit_file",
        }
    }
}

pub fn validate_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(PathScanError::SymlinkNotAllowed(path.to_path_buf()).into());
    }
    if metadata.is_file() && metadata.len() > MAX_FILE_SIZE_BYTES {
        bail!("file exceeds max size: {}", path.display());
    }
    Ok(())
}

fn classify_path_error(error: &anyhow::Error, result: &mut IndexResult) {
    if let Some(path_error) = error.downcast_ref::<PathScanError>() {
        match path_error {
            PathScanError::SymlinkNotAllowed(_) => result.skipped_symlinks += 1,
            PathScanError::UnsafePath(_) => result.skipped_unsafe_paths += 1,
        }
    }
}

fn reject_broad_scan_path(path: &Path) -> Result<()> {
    let canonical = path.canonicalize()?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let cwd = std::env::current_dir().ok();
    if canonical == Path::new("/")
        || home.as_ref().is_some_and(|value| &canonical == value)
        || cwd.as_ref().is_some_and(|value| &canonical == value)
    {
        return Err(PathScanError::UnsafePath(canonical).into());
    }
    if canonical.is_dir() && !is_known_transcript_root(&canonical) && !test_temp_path(&canonical) {
        return Err(PathScanError::UnsafePath(canonical).into());
    }
    if canonical.is_file() && !supported_discovered_file(&canonical) {
        return Err(PathScanError::UnsafePath(canonical).into());
    }
    Ok(())
}

#[derive(Debug)]
enum PathScanError {
    SymlinkNotAllowed(PathBuf),
    UnsafePath(PathBuf),
}

impl std::fmt::Display for PathScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SymlinkNotAllowed(path) => {
                write!(f, "symlinks are not allowed: {}", path.display())
            }
            Self::UnsafePath(path) => write!(
                f,
                "unsafe transcript scan path: {}; pass a known transcript root or one .jsonl file",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PathScanError {}

fn is_known_transcript_root(path: &Path) -> bool {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return false;
    };
    let allowed = [home.join(".claude/projects"), home.join(".codex/sessions")];
    allowed
        .iter()
        .any(|root| path == root || path.starts_with(root))
}

fn test_temp_path(path: &Path) -> bool {
    cfg!(test) && path.starts_with(std::env::temp_dir())
}

pub fn index_roots(pool: &DbPool, root_override: Option<&Path>) -> Result<IndexResult> {
    index_roots_with_storage(pool, root_override, None)
}

pub fn index_roots_with_storage(
    pool: &DbPool,
    root_override: Option<&Path>,
    storage: Option<&StorageConfig>,
) -> Result<IndexResult> {
    let roots = match root_override {
        Some(path) => {
            let mut result = IndexResult::default();
            if let Err(error) = validate_path(path).and_then(|_| reject_broad_scan_path(path)) {
                classify_path_error(&error, &mut result);
                record_file_error(&mut result, path, &error);
                return Ok(result);
            }
            vec![path.to_path_buf()]
        }
        None => default_roots(),
    };

    let mut result = IndexResult::default();
    let mut files = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        collect_supported_files(&root, &mut files, &mut result);
    }
    files.sort();
    files.dedup();
    for file in files {
        match index_file_with_storage(pool, &file, detect_source_kind(&file).as_str(), storage) {
            Ok(file_result) => merge_result(&mut result, &file_result),
            Err(error) => {
                classify_path_error(&error, &mut result);
                tracing::warn!(path = %file.display(), error = %error, "Transcript file indexing failed");
                record_file_error(&mut result, &file, &error);
            }
        }
    }
    Ok(result)
}

pub fn index_file(pool: &DbPool, path: &Path, source_kind: &str) -> Result<IndexResult> {
    index_file_with_storage(pool, path, source_kind, None)
}

pub fn index_file_with_storage(
    pool: &DbPool,
    path: &Path,
    source_kind: &str,
    storage: Option<&StorageConfig>,
) -> Result<IndexResult> {
    validate_path(path)?;
    if !path.is_file() {
        bail!("expected a file path: {}", path.display());
    }
    if let Some(storage) = storage {
        let outcome = enforce_storage_budget(pool, storage)?;
        if outcome.write_blocked {
            return Ok(IndexResult {
                discovered_files: 1,
                storage_blocked_chunks: 1,
                ..Default::default()
            });
        }
    }

    let canonical_path = path.canonicalize()?;
    let canonical = canonical_path.to_string_lossy().to_string();
    let source_kind = SourceKind::from_str(source_kind, &canonical_path);
    let tool = source_kind.tool_name();
    let mut fallback_project = project_for_file(source_kind, &canonical_path);
    let mut fallback_session_id = if source_kind == SourceKind::CodexSession {
        canonical_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToString::to_string)
    } else {
        None
    };
    let checkpoint_store = checkpoint::CheckpointStore::new(pool);
    let source_id = checkpoint_store.ensure_source(&canonical, source_kind.as_str())?;
    let current_metadata = FileMetadata::from_path_metadata(&canonical_path)?;
    if source_kind != SourceKind::ExplicitFile
        && checkpoint_store.source_matches_metadata(
            source_id,
            current_metadata.size,
            current_metadata.mtime,
        )?
    {
        return Ok(IndexResult {
            discovered_files: 1,
            ..Default::default()
        });
    }
    let mut imports = Vec::new();
    let mut batch = Vec::new();
    let mut chunk_bytes = 0usize;
    let file = fs::File::open(&canonical_path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut line_no = 0usize;
    let mut result = IndexResult {
        discovered_files: 1,
        ..Default::default()
    };

    loop {
        let Some(read_line) = read_bounded_line(&mut reader, &mut hasher)? else {
            break;
        };
        if read_line.oversized {
            result.parse_errors += 1;
            checkpoint_store.mark_error(source_id, "transcript record exceeds max size")?;
            line_no += 1;
            continue;
        }
        let line_text = read_line.text.trim_end_matches(['\r', '\n']);
        if line_text.trim().is_empty() {
            line_no += 1;
            continue;
        }
        update_codex_fallbacks(
            source_kind,
            line_text,
            &mut fallback_project,
            &mut fallback_session_id,
        );
        match parse_line_for_source(source_kind, line_text, &canonical_path, line_no) {
            Ok(Some(parsed)) => {
                let record_key = parsed.record_key;
                let message = scrub_ai_message(&parsed.message, None);
                let project = accept_metadata_field(
                    parsed.ai_project.as_deref().or(fallback_project.as_deref()),
                    MAX_AI_PROJECT_CHARS,
                    "ai_project",
                    &mut result,
                );
                let session_id = accept_metadata_field(
                    parsed
                        .session_id
                        .as_deref()
                        .or(fallback_session_id.as_deref()),
                    MAX_AI_SESSION_ID_CHARS,
                    "ai_session_id",
                    &mut result,
                );
                let transcript_path = accept_metadata_field(
                    Some(&canonical),
                    MAX_TRANSCRIPT_PATH_CHARS,
                    "ai_transcript_path",
                    &mut result,
                );
                let entry = LogBatchEntry {
                    timestamp: normalize_timestamp(parsed.timestamp.as_deref())?,
                    hostname: "localhost".to_string(),
                    facility: Some("transcript".to_string()),
                    severity: "info".to_string(),
                    app_name: Some(format!("{tool}-transcript")),
                    process_id: None,
                    raw: message.clone(),
                    message,
                    source_ip: format!("transcript://{}", source_kind.as_str()),
                    docker_checkpoint: None,
                    ai_tool: Some(tool.to_string()),
                    ai_project: project,
                    ai_session_id: session_id,
                    ai_transcript_path: transcript_path,
                };
                chunk_bytes = chunk_bytes.saturating_add(log_entry_string_bytes(&entry));
                batch.push(entry);
                imports.push(record_key);
                if batch.len() >= MAX_INDEX_CHUNK_RECORDS || chunk_bytes >= MAX_INDEX_CHUNK_BYTES {
                    if !flush_chunk(
                        pool,
                        storage,
                        source_id,
                        &mut batch,
                        &mut imports,
                        None,
                        &mut result,
                    )? {
                        return Ok(result);
                    }
                    chunk_bytes = 0;
                }
            }
            Ok(None) => {}
            Err(error) => {
                result.parse_errors += 1;
                checkpoint_store.mark_error(source_id, &error.to_string())?;
            }
        }
        line_no += 1;
    }

    let file_metadata = FileMetadata::from_path_hash(&canonical_path, &hasher.finalize())?;
    if result.parse_errors > 0 {
        flush_chunk(
            pool,
            storage,
            source_id,
            &mut batch,
            &mut imports,
            None,
            &mut result,
        )?;
        checkpoint_store.mark_error(
            source_id,
            &format!(
                "{} transcript record(s) failed to parse",
                result.parse_errors
            ),
        )?;
        return Ok(result);
    }
    let _ = flush_chunk(
        pool,
        storage,
        source_id,
        &mut batch,
        &mut imports,
        Some(&file_metadata),
        &mut result,
    )?;
    Ok(result)
}

fn flush_chunk(
    pool: &DbPool,
    storage: Option<&StorageConfig>,
    source_id: i64,
    batch: &mut Vec<LogBatchEntry>,
    imports: &mut Vec<String>,
    completion_metadata: Option<&FileMetadata>,
    result: &mut IndexResult,
) -> Result<bool> {
    if batch.is_empty() {
        if let Some(file_metadata) = completion_metadata {
            let mut conn = pool.get()?;
            let tx = conn.transaction()?;
            checkpoint::update_source_metadata_in_tx(&tx, source_id, file_metadata)?;
            tx.commit()?;
            result.checkpoint_updates += 1;
        }
        return Ok(true);
    }

    if let Some(storage) = storage {
        let outcome = enforce_storage_budget(pool, storage)?;
        if outcome.write_blocked {
            result.storage_blocked_chunks += 1;
            batch.clear();
            imports.clear();
            return Ok(false);
        }
    }

    let mut conn = pool.get()?;
    let tx = conn.transaction()?;
    let claimed = checkpoint::claim_imports_in_tx(&tx, source_id, imports)?;
    let mut claimed_batch = Vec::with_capacity(batch.len());
    let mut skipped_dupes = 0usize;
    for (entry, claimed) in batch.iter().cloned().zip(claimed) {
        if claimed {
            claimed_batch.push(entry);
        } else {
            skipped_dupes += 1;
        }
    }
    if !claimed_batch.is_empty() {
        insert_logs_batch_in_tx(&tx, &claimed_batch)?;
    }
    if let Some(file_metadata) = completion_metadata {
        checkpoint::update_source_metadata_in_tx(&tx, source_id, file_metadata)?;
    }
    tx.commit()?;
    result.ingested += claimed_batch.len();
    result.skipped_dupes += skipped_dupes;
    if completion_metadata.is_some() {
        result.checkpoint_updates += 1;
    }
    batch.clear();
    imports.clear();
    Ok(true)
}

fn collect_supported_files(path: &Path, files: &mut Vec<PathBuf>, result: &mut IndexResult) {
    if let Err(error) = validate_path(path) {
        classify_path_error(&error, result);
        record_file_error(result, path, &error);
        return;
    }
    if path.is_file() {
        if supported_discovered_file(path) {
            files.push(path.to_path_buf());
        } else {
            result.unsupported_files += 1;
        }
        return;
    }

    let mut entries = Vec::new();
    let read_dir = match fs::read_dir(path) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            record_discovered_path_error(result, path, &error.into());
            return;
        }
    };
    for entry in read_dir {
        match entry.with_context(|| format!("failed to read entry under {}", path.display())) {
            Ok(entry) => entries.push(entry.path()),
            Err(error) => {
                record_discovered_path_error(result, path, &error);
            }
        }
    }
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            collect_supported_files(&entry, files, result);
        } else if supported_discovered_file(&entry) {
            files.push(entry);
        } else {
            result.unsupported_files += 1;
        }
    }
}

fn supported_discovered_file(path: &Path) -> bool {
    matches!(path.extension().and_then(|ext| ext.to_str()), Some("jsonl"))
}

fn detect_source_kind(path: &Path) -> SourceKind {
    let display = path.to_string_lossy();
    if display.contains(".codex/sessions") {
        SourceKind::CodexSession
    } else if display.contains(".claude/projects") {
        SourceKind::ClaudeProject
    } else {
        SourceKind::ExplicitFile
    }
}

impl SourceKind {
    fn from_str(source_kind: &str, path: &Path) -> Self {
        match source_kind {
            "codex_session" => Self::CodexSession,
            "claude_project" => Self::ClaudeProject,
            _ => detect_source_kind(path),
        }
    }

    fn tool_name(self) -> &'static str {
        match self {
            Self::CodexSession => "codex",
            Self::ClaudeProject | Self::ExplicitFile => "claude",
        }
    }
}

fn parse_line_for_source(
    source_kind: SourceKind,
    line: &str,
    path: &Path,
    line_no: usize,
) -> Result<Option<ParsedTranscriptRecord>> {
    match source_kind {
        SourceKind::CodexSession => codex::parse_line(line, path, line_no),
        SourceKind::ClaudeProject | SourceKind::ExplicitFile => {
            claude::parse_line(line, path, line_no)
        }
    }
}

fn project_for_file(source_kind: SourceKind, path: &Path) -> Option<String> {
    match source_kind {
        SourceKind::ClaudeProject => project_from_transcript_path(&path.to_string_lossy()),
        SourceKind::CodexSession => None,
        SourceKind::ExplicitFile => std::env::current_dir()
            .ok()
            .map(|path| path.to_string_lossy().to_string()),
    }
}

fn update_codex_fallbacks(
    source_kind: SourceKind,
    line: &str,
    fallback_project: &mut Option<String>,
    fallback_session_id: &mut Option<String>,
) {
    if source_kind != SourceKind::CodexSession {
        return;
    }
    if let Some(project) = codex::project_from_line(line) {
        *fallback_project = Some(project);
    }
    if let Some(session_id) = codex::session_id_from_line(line) {
        *fallback_session_id = Some(session_id);
    }
}

fn merge_result(total: &mut IndexResult, next: &IndexResult) {
    total.discovered_files += next.discovered_files;
    total.ingested += next.ingested;
    total.skipped_dupes += next.skipped_dupes;
    total.parse_errors += next.parse_errors;
    total.skipped_files += next.skipped_files;
    total.unsupported_files += next.unsupported_files;
    total.skipped_symlinks += next.skipped_symlinks;
    total.skipped_unsafe_paths += next.skipped_unsafe_paths;
    total.storage_blocked_chunks += next.storage_blocked_chunks;
    total.dropped_metadata_fields += next.dropped_metadata_fields;
    total.checkpoint_updates += next.checkpoint_updates;
    total.file_errors.extend(next.file_errors.iter().cloned());
}

fn record_file_error(result: &mut IndexResult, path: &Path, error: &anyhow::Error) {
    result.skipped_files += 1;
    result.file_errors.push(IndexFileError {
        path: path.display().to_string(),
        error: error.to_string(),
    });
}

fn record_discovered_path_error(result: &mut IndexResult, path: &Path, error: &anyhow::Error) {
    result.skipped_files += 1;
    if is_permission_denied(error) {
        tracing::debug!(
            path = %path.display(),
            error = %error,
            "Skipping unreadable transcript path during discovery"
        );
    } else {
        result.file_errors.push(IndexFileError {
            path: path.display().to_string(),
            error: error.to_string(),
        });
    }
}

fn is_permission_denied(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
}

fn accept_metadata_field(
    value: Option<&str>,
    max_chars: usize,
    field: &'static str,
    result: &mut IndexResult,
) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() > max_chars {
        result.dropped_metadata_fields += 1;
        tracing::debug!(
            field,
            max_chars,
            actual_chars = trimmed.chars().count(),
            "Dropping oversized AI transcript metadata field"
        );
        return None;
    }
    Some(trimmed.to_string())
}

fn normalize_timestamp(timestamp: Option<&str>) -> Result<String> {
    match timestamp {
        Some(value) => Ok(chrono::DateTime::parse_from_rfc3339(value)
            .with_context(|| format!("invalid transcript timestamp: {value}"))?
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()),
        None => Ok(chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()),
    }
}

fn log_entry_string_bytes(entry: &LogBatchEntry) -> usize {
    entry
        .timestamp
        .len()
        .saturating_add(entry.hostname.len())
        .saturating_add(entry.facility.as_ref().map_or(0, String::len))
        .saturating_add(entry.severity.len())
        .saturating_add(entry.app_name.as_ref().map_or(0, String::len))
        .saturating_add(entry.process_id.as_ref().map_or(0, String::len))
        .saturating_add(entry.raw.len())
        .saturating_add(entry.message.len())
        .saturating_add(entry.source_ip.len())
        .saturating_add(entry.docker_checkpoint.as_ref().map_or(0, |checkpoint| {
            checkpoint
                .host_name
                .len()
                .saturating_add(checkpoint.container_id.len())
                .saturating_add(checkpoint.timestamp.len())
        }))
        .saturating_add(entry.ai_tool.as_ref().map_or(0, String::len))
        .saturating_add(entry.ai_project.as_ref().map_or(0, String::len))
        .saturating_add(entry.ai_session_id.as_ref().map_or(0, String::len))
        .saturating_add(entry.ai_transcript_path.as_ref().map_or(0, String::len))
}

struct ReadLine {
    text: String,
    oversized: bool,
}

fn read_bounded_line<R: BufRead>(reader: &mut R, hasher: &mut Sha256) -> Result<Option<ReadLine>> {
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if line.is_empty() && !oversized {
                return Ok(None);
            }
            break;
        }
        let newline_pos = available.iter().position(|byte| *byte == b'\n');
        let take_len = newline_pos.map_or(available.len(), |pos| pos + 1);
        hasher.update(&available[..take_len]);
        if !oversized {
            // Copy one sentinel byte past the inclusive record limit so we can
            // detect oversized records while still accepting exactly-limit rows.
            let remaining = MAX_RECORD_SIZE_BYTES
                .saturating_add(1)
                .saturating_sub(line.len());
            let copy_len = take_len.min(remaining);
            line.extend_from_slice(&available[..copy_len]);
            if line.len() > MAX_RECORD_SIZE_BYTES {
                oversized = true;
                line.clear();
            }
        }
        reader.consume(take_len);
        if newline_pos.is_some() {
            break;
        }
    }
    if oversized {
        return Ok(Some(ReadLine {
            text: String::new(),
            oversized: true,
        }));
    }
    let text = String::from_utf8(line).context("transcript record is not valid UTF-8")?;
    Ok(Some(ReadLine {
        text,
        oversized: false,
    }))
}

fn default_roots() -> Vec<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| vec![home.join(".claude/projects"), home.join(".codex/sessions")])
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "scanner_tests.rs"]
mod tests;

#[derive(Debug, Clone)]
pub(crate) struct ParsedTranscriptRecord {
    pub record_key: String,
    pub timestamp: Option<String>,
    pub message: String,
    pub session_id: Option<String>,
    pub ai_project: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FileMetadata {
    pub size: u64,
    pub mtime: Option<i64>,
    pub content_hash: String,
}

impl FileMetadata {
    fn from_path_metadata(path: &Path) -> Result<Self> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            size: metadata.len(),
            mtime: metadata_mtime_secs(&metadata),
            content_hash: String::new(),
        })
    }

    fn from_path_hash(path: &Path, hash: &[u8]) -> Result<Self> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            size: metadata.len(),
            mtime: metadata_mtime_secs(&metadata),
            content_hash: hex_digest(hash),
        })
    }
}

fn metadata_mtime_secs(metadata: &fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn record_key_from_line(
    value: &serde_json::Value,
    line: &str,
    line_no: usize,
) -> String {
    value
        .get("uuid")
        .or_else(|| value.get("id"))
        .or_else(|| value.pointer("/payload/id"))
        .and_then(serde_json::Value::as_str)
        .map(|id| format!("id:{id}"))
        .unwrap_or_else(|| format!("line:{line_no}:hash:{}", hash_text(line)))
}

pub(crate) fn hash_text(text: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}
