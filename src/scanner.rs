use std::collections::HashSet;
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

const MAX_FILE_SIZE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_RECORD_SIZE_BYTES: usize = 64 * 1024;
const MAX_INDEX_CHUNK_RECORDS: usize = 500;
const MAX_INDEX_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const MAX_AI_FIELD_CHARS: usize = 512;
const MAX_TRANSCRIPT_PATH_CHARS: usize = 2048;

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
        bail!("symlinks are not allowed: {}", path.display());
    }
    if metadata.is_file() && metadata.len() > MAX_FILE_SIZE_BYTES {
        bail!("file exceeds max size: {}", path.display());
    }
    Ok(())
}

fn classify_path_error(error: &anyhow::Error, result: &mut IndexResult) {
    let message = error.to_string();
    if message.contains("symlinks are not allowed") {
        result.skipped_symlinks += 1;
    }
    if message.contains("unsafe transcript scan path") {
        result.skipped_unsafe_paths += 1;
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
        bail!("unsafe transcript scan path: {}", canonical.display());
    }
    Ok(())
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
                result.skipped_files += 1;
                result.file_errors.push(IndexFileError {
                    path: path.display().to_string(),
                    error: error.to_string(),
                });
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
                result.skipped_files += 1;
                result.file_errors.push(IndexFileError {
                    path: file.display().to_string(),
                    error: error.to_string(),
                });
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
    let existing_keys = checkpoint_store.record_keys(source_id)?;
    let mut seen_keys = HashSet::new();
    let mut imports = Vec::new();
    let mut batch = Vec::new();
    let mut chunk_bytes = 0usize;
    let file = fs::File::open(&canonical_path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut line = String::new();
    let mut line_no = 0usize;
    let mut result = IndexResult {
        discovered_files: 1,
        ..Default::default()
    };

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(line.as_bytes());
        let line_text = line.trim_end_matches(['\r', '\n']);
        if line_text.trim().is_empty() {
            line_no += 1;
            continue;
        }
        if line_text.len() > MAX_RECORD_SIZE_BYTES {
            result.parse_errors += 1;
            checkpoint_store.mark_error(source_id, "transcript record exceeds max size")?;
            line_no += 1;
            continue;
        }
        if source_kind == SourceKind::CodexSession {
            fallback_project =
                codex::project_from_line(line_text).or_else(|| fallback_project.clone());
            fallback_session_id =
                codex::session_id_from_line(line_text).or_else(|| fallback_session_id.clone());
        }
        match parse_line_for_source(source_kind, line_text, &canonical_path, line_no) {
            Ok(Some(parsed)) => {
                let record_key = parsed.record_key;
                if existing_keys.contains(&record_key) || !seen_keys.insert(record_key.clone()) {
                    result.skipped_dupes += 1;
                    continue;
                }
                let message = scrub_ai_message(&parsed.message, None);
                let project = parsed
                    .ai_project
                    .as_deref()
                    .and_then(|value| cap_field(value, MAX_AI_FIELD_CHARS))
                    .or_else(|| fallback_project.clone());
                let session_id_source = parsed.session_id.or_else(|| fallback_session_id.clone());
                let session_id = session_id_source
                    .as_deref()
                    .and_then(|value| cap_field(value, MAX_AI_FIELD_CHARS));
                let transcript_path = cap_field(&canonical, MAX_TRANSCRIPT_PATH_CHARS);
                chunk_bytes = chunk_bytes.saturating_add(message.len());
                batch.push(LogBatchEntry {
                    timestamp: parsed.timestamp.unwrap_or_else(|| {
                        chrono::Utc::now()
                            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                            .to_string()
                    }),
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
                });
                imports.push(record_key);
                if batch.len() >= MAX_INDEX_CHUNK_RECORDS || chunk_bytes >= MAX_INDEX_CHUNK_BYTES {
                    flush_chunk(
                        pool,
                        storage,
                        source_id,
                        &mut batch,
                        &mut imports,
                        &FileMetadata::partial(&canonical_path)?,
                        &mut result,
                    )?;
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
    flush_chunk(
        pool,
        storage,
        source_id,
        &mut batch,
        &mut imports,
        &file_metadata,
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
    file_metadata: &FileMetadata,
    result: &mut IndexResult,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    if let Some(storage) = storage {
        let outcome = enforce_storage_budget(pool, storage)?;
        if outcome.write_blocked {
            result.storage_blocked_chunks += 1;
            batch.clear();
            imports.clear();
            return Ok(());
        }
    }

    let mut conn = pool.get()?;
    let tx = conn.transaction()?;
    insert_logs_batch_in_tx(&tx, batch)?;
    checkpoint::record_imports_in_tx(&tx, source_id, imports, file_metadata)?;
    tx.commit()?;
    result.ingested += batch.len();
    result.checkpoint_updates += 1;
    batch.clear();
    imports.clear();
    Ok(())
}

fn collect_supported_files(path: &Path, files: &mut Vec<PathBuf>, result: &mut IndexResult) {
    if let Err(error) = validate_path(path) {
        classify_path_error(&error, result);
        result.skipped_files += 1;
        result.file_errors.push(IndexFileError {
            path: path.display().to_string(),
            error: error.to_string(),
        });
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
            result.skipped_files += 1;
            result.file_errors.push(IndexFileError {
                path: path.display().to_string(),
                error: error.to_string(),
            });
            return;
        }
    };
    for entry in read_dir {
        match entry.with_context(|| format!("failed to read entry under {}", path.display())) {
            Ok(entry) => entries.push(entry.path()),
            Err(error) => {
                result.skipped_files += 1;
                result.file_errors.push(IndexFileError {
                    path: path.display().to_string(),
                    error: error.to_string(),
                });
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
    total.checkpoint_updates += next.checkpoint_updates;
    total.file_errors.extend(next.file_errors.iter().cloned());
}

fn cap_field(value: &str, max_chars: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut out = String::new();
    for ch in trimmed.chars().take(max_chars) {
        out.push(ch);
    }
    Some(out)
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
    fn partial(path: &Path) -> Result<Self> {
        Self::from_path_hash(path, &Sha256::new().finalize())
    }

    fn from_path_hash(path: &Path, hash: &[u8]) -> Result<Self> {
        let metadata = fs::metadata(path)?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64);
        Ok(Self {
            size: metadata.len(),
            mtime,
            content_hash: hex_digest(hash),
        })
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn record_key_from_line(value: &serde_json::Value, line: &str) -> String {
    value
        .get("uuid")
        .or_else(|| value.get("id"))
        .or_else(|| value.pointer("/payload/id"))
        .or_else(|| value.pointer("/session/id"))
        .and_then(serde_json::Value::as_str)
        .map(|id| format!("id:{id}"))
        .unwrap_or_else(|| format!("hash:{}", hash_text(line)))
}

pub(crate) fn hash_text(text: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}
