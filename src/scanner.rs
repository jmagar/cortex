use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::db::{insert_logs_batch_in_tx, DbPool, LogBatchEntry};
use crate::syslog::enrichment::{project_from_transcript_path, scrub_ai_message};

mod checkpoint;
mod claude;
mod codex;

pub use checkpoint::CheckpointStore;

const MAX_FILE_SIZE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct IndexResult {
    pub discovered_files: usize,
    pub ingested: usize,
    pub skipped_dupes: usize,
    pub parse_errors: usize,
    pub skipped_files: usize,
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

pub fn index_roots(pool: &DbPool, root_override: Option<&Path>) -> Result<IndexResult> {
    let roots = match root_override {
        Some(path) => vec![path.to_path_buf()],
        None => default_roots(),
    };

    let mut result = IndexResult::default();
    let mut files = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        collect_supported_files(&root, &mut files)?;
    }
    files.sort();
    files.dedup();
    for file in files {
        match index_file(pool, &file, detect_source_kind(&file).as_str()) {
            Ok(file_result) => merge_result(&mut result, &file_result),
            Err(error) => {
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
    validate_path(path)?;
    if !path.is_file() {
        bail!("expected a file path: {}", path.display());
    }

    let canonical_path = path.canonicalize()?;
    let canonical = canonical_path.to_string_lossy().to_string();
    let source_kind = SourceKind::from_str(source_kind, &canonical_path);
    let tool = source_kind.tool_name();
    let mut fallback_project = project_for_file(source_kind, &canonical_path);
    let checkpoint_store = checkpoint::CheckpointStore::new(pool);
    let source_id = checkpoint_store.ensure_source(&canonical, source_kind.as_str())?;
    let existing_keys = checkpoint_store.record_keys(source_id)?;
    let content = fs::read_to_string(&canonical_path)?;
    let file_metadata = FileMetadata::from_path(&canonical_path, &content)?;
    let mut seen_keys = HashSet::new();
    let mut imports = Vec::new();
    let mut batch = Vec::new();
    let mut result = IndexResult {
        discovered_files: 1,
        ..Default::default()
    };

    for (line_no, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if source_kind == SourceKind::CodexSession {
            fallback_project = codex::project_from_line(line).or_else(|| fallback_project.clone());
        }
        match parse_line_for_source(source_kind, line, &canonical_path, line_no) {
            Ok(Some(parsed)) => {
                let record_key = parsed.record_key;
                if existing_keys.contains(&record_key) || !seen_keys.insert(record_key.clone()) {
                    result.skipped_dupes += 1;
                    continue;
                }
                let message = scrub_ai_message(&parsed.message, None);
                let project = parsed
                    .ai_project
                    .clone()
                    .or_else(|| fallback_project.clone());
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
                    ai_session_id: parsed.session_id,
                    ai_transcript_path: Some(canonical.clone()),
                });
                imports.push(record_key);
            }
            Ok(None) => {}
            Err(error) => {
                result.parse_errors += 1;
                checkpoint_store.mark_error(source_id, &error.to_string())?;
            }
        }
    }

    if batch.is_empty() {
        return Ok(result);
    }

    let mut conn = pool.get()?;
    let tx = conn.transaction()?;
    insert_logs_batch_in_tx(&tx, &batch)?;
    checkpoint::record_imports_in_tx(&tx, source_id, &imports, &file_metadata)?;
    tx.commit()?;
    result.ingested = batch.len();
    Ok(result)
}

fn collect_supported_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    validate_path(path)?;
    if path.is_file() {
        if supported_file(path) {
            files.push(path.to_path_buf());
        }
        return Ok(());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry =
            entry.with_context(|| format!("failed to read entry under {}", path.display()))?;
        entries.push(entry.path());
    }
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            collect_supported_files(&entry, files)?;
        } else if supported_file(&entry) {
            files.push(entry);
        }
    }
    Ok(())
}

fn supported_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("jsonl") | Some("json")
    )
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
    total.file_errors.extend(next.file_errors.iter().cloned());
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
    fn from_path(path: &Path, content: &str) -> Result<Self> {
        let metadata = fs::metadata(path)?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64);
        Ok(Self {
            size: metadata.len(),
            mtime,
            content_hash: hash_text(content),
        })
    }
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
