use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::db::{insert_logs_batch, DbPool, LogBatchEntry};

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
    result.discovered_files = files.len();
    for file in files {
        match index_file(pool, &file, detect_source_kind(&file)) {
            Ok(file_result) => merge_result(&mut result, &file_result),
            Err(_) => result.skipped_files += 1,
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
    let checkpoint_store = checkpoint::CheckpointStore::new(pool);
    let source_id = checkpoint_store.ensure_source(&canonical, source_kind)?;
    let content = fs::read_to_string(&canonical_path)?;
    let mut record_keys = Vec::new();
    let mut batch = Vec::new();
    let mut result = IndexResult {
        discovered_files: 1,
        ..Default::default()
    };

    for (line_no, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match claude::parse_line(line, &canonical_path, line_no) {
            Ok(Some(parsed)) => {
                let record_key = parsed.record_key;
                if checkpoint_store.has_record(source_id, &record_key)? {
                    result.skipped_dupes += 1;
                    continue;
                }
                batch.push(LogBatchEntry {
                    timestamp: parsed.timestamp.unwrap_or_else(|| {
                        chrono::Utc::now()
                            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                            .to_string()
                    }),
                    hostname: "localhost".to_string(),
                    facility: Some("transcript".to_string()),
                    severity: "info".to_string(),
                    app_name: Some("ai-transcript".to_string()),
                    process_id: None,
                    raw: parsed.message.clone(),
                    message: parsed.message,
                    source_ip: format!("transcript://{}", source_kind),
                    docker_checkpoint: None,
                    ai_tool: Some(detect_tool(&canonical_path, source_kind)),
                    ai_project: canonical_path
                        .parent()
                        .map(|parent| parent.to_string_lossy().to_string()),
                    ai_session_id: parsed.session_id,
                    ai_transcript_path: Some(canonical.clone()),
                });
                record_keys.push(record_key);
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

    insert_logs_batch(pool, &batch)?;
    checkpoint_store.record_imports(source_id, &record_keys)?;
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

    let mut entries: Vec<PathBuf> = fs::read_dir(path)?
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .collect();
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

fn detect_source_kind(path: &Path) -> &'static str {
    let display = path.to_string_lossy();
    if display.contains(".codex/sessions") {
        "codex_session"
    } else if display.contains(".claude/projects") {
        "claude_project"
    } else {
        "explicit_file"
    }
}

fn detect_tool(path: &Path, source_kind: &str) -> String {
    let display = path.to_string_lossy();
    if display.contains(".codex/sessions") || source_kind == "codex_session" {
        codex::tool_name().to_string()
    } else {
        "claude".to_string()
    }
}

fn merge_result(total: &mut IndexResult, next: &IndexResult) {
    total.discovered_files += next.discovered_files;
    total.ingested += next.ingested;
    total.skipped_dupes += next.skipped_dupes;
    total.parse_errors += next.parse_errors;
    total.skipped_files += next.skipped_files;
}

fn default_roots() -> Vec<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| vec![home.join(".claude/projects"), home.join(".codex/sessions")])
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../scanner_tests.rs"]
mod tests;
