use std::collections::HashMap;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncSeekExt, BufReader};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::db::LogBatchEntry;
use crate::enrich::{SourceKind, stamp_source_kind};
use crate::ingest::IngestTx;
use crate::ingest_metadata::bounded_metadata_json;

use super::models::{FileTailSource, FileTailStatus};
use super::path_policy::{validate_file_tail_path, validate_opened_file_tail_path};
use super::registry::FileTailRegistry;

#[derive(Clone)]
pub(crate) struct FileTailSupervisor {
    registry: Arc<FileTailRegistry>,
    ingest: IngestTx,
    token: CancellationToken,
    tasks: Arc<Mutex<HashMap<String, TailTask>>>,
    max_line_bytes: usize,
}

struct TailTask {
    handle: JoinHandle<()>,
    status: Arc<Mutex<FileTailStatus>>,
    source: FileTailSource,
}

impl FileTailSupervisor {
    pub(crate) fn new(
        registry: Arc<FileTailRegistry>,
        ingest: IngestTx,
        token: CancellationToken,
        max_line_bytes: usize,
    ) -> Self {
        Self {
            registry,
            ingest,
            token,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            max_line_bytes,
        }
    }

    pub(crate) fn statuses(&self) -> Vec<FileTailStatus> {
        let mut out: Vec<_> = self
            .tasks
            .lock()
            .values()
            .map(|task| task.status.lock().clone())
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    pub(crate) fn shutdown(&self) {
        self.token.cancel();
        let mut tasks = self.tasks.lock();
        for (_, task) in tasks.drain() {
            task.status.lock().running = false;
            task.handle.abort();
        }
    }

    pub(crate) fn reconcile(&self) -> Result<()> {
        let sources = self.registry.list()?;
        let enabled: HashMap<String, FileTailSource> = sources
            .iter()
            .filter(|source| source.enabled)
            .map(|source| (source.id.clone(), source.clone()))
            .collect();

        let to_spawn = {
            let mut tasks = self.tasks.lock();
            tasks.retain(|id, task| {
                let keep_running = enabled
                    .get(id)
                    .is_some_and(|source| source.same_definition(&task.source));
                if !keep_running {
                    task.status.lock().running = false;
                    task.handle.abort();
                }
                keep_running
            });
            sources
                .into_iter()
                .filter(|source| source.enabled && !tasks.contains_key(&source.id))
                .collect::<Vec<_>>()
        };

        for source in to_spawn {
            self.spawn_source(source);
        }
        Ok(())
    }

    fn spawn_source(&self, source: FileTailSource) {
        let id = source.id.clone();
        let task_source = source.clone();
        let status = Arc::new(Mutex::new(FileTailStatus {
            id: id.clone(),
            running: true,
            last_line_at: None,
            last_error: None,
        }));
        let task_status = Arc::clone(&status);
        let ingest = self.ingest.clone();
        let token = self.token.clone();
        let registry = Arc::clone(&self.registry);
        let max_line_bytes = self.max_line_bytes;
        let task_id = id.clone();
        let handle = tokio::spawn(async move {
            tail_file_loop(
                task_id,
                registry,
                ingest,
                token,
                task_status,
                max_line_bytes,
            )
            .await;
        });
        self.tasks.lock().insert(
            id,
            TailTask {
                handle,
                status,
                source: task_source,
            },
        );
    }

    #[cfg(test)]
    pub(crate) fn running_source_for_test(&self, id: &str) -> Option<FileTailSource> {
        self.tasks.lock().get(id).map(|task| task.source.clone())
    }
}

async fn tail_file_loop(
    source_id: String,
    registry: Arc<FileTailRegistry>,
    ingest: IngestTx,
    token: CancellationToken,
    status: Arc<Mutex<FileTailStatus>>,
    max_line_bytes: usize,
) {
    loop {
        if token.is_cancelled() {
            status.lock().running = false;
            return;
        }
        let source = match registry.get(&source_id) {
            Ok(Some(source)) if source.enabled => source,
            Ok(_) => {
                status.lock().running = false;
                return;
            }
            Err(err) => {
                tracing::error!(
                    source_id = %source_id,
                    error = %err,
                    "file-tail source reload failed; retrying"
                );
                status.lock().last_error = Some(err.to_string());
                tokio::select! {
                    _ = token.cancelled() => {
                        status.lock().running = false;
                        return;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                }
                continue;
            }
        };
        match tail_file_until_cancelled(
            &source,
            Arc::clone(&registry),
            ingest.clone(),
            token.clone(),
            Arc::clone(&status),
            max_line_bytes,
        )
        .await
        {
            Ok(()) => {
                status.lock().running = false;
                return;
            }
            Err(err) => {
                tracing::error!(
                    source_id = %source.id,
                    path = %source.path,
                    error = %err,
                    "file-tail source failed; retrying"
                );
                status.lock().last_error = Some(err.to_string());
                tokio::select! {
                    _ = token.cancelled() => {
                        status.lock().running = false;
                        return;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                }
            }
        }
    }
}

async fn tail_file_until_cancelled(
    source: &FileTailSource,
    registry: Arc<FileTailRegistry>,
    ingest: IngestTx,
    token: CancellationToken,
    status: Arc<Mutex<FileTailStatus>>,
    max_line_bytes: usize,
) -> Result<()> {
    let opened = open_tail_file(source, true)
        .await
        .with_context(|| format!("open {}", source.path))?;
    let mut reader = BufReader::new(opened.file);
    let mut position = opened.position;
    let mut identity = opened.identity;
    let mut line = Vec::new();
    loop {
        tokio::select! {
            _ = token.cancelled() => return Ok(()),
            read = read_bounded_line(&mut reader, &mut line, max_line_bytes) => {
                let read = read?;
                if read.bytes_read == 0 {
                    if let Some(next) = reopen_if_rotated_or_truncated(source, identity, position).await? {
                        if !line.is_empty() {
                            let now = now_iso();
                            let partial = PartialLineBeforeReopen {
                                source,
                                registry: &registry,
                                ingest: &ingest,
                                status: &status,
                                line: &line,
                                identity,
                                position,
                                now: &now,
                            };
                            ingest_partial_line_before_reopen(partial).await?;
                        }
                        reader = BufReader::new(next.file);
                        position = next.position;
                        identity = next.identity;
                        line.clear();
                    } else {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    continue;
                }
                position = position.saturating_add(read.bytes_read as u64);
                if !read.complete {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                let msg = String::from_utf8_lossy(&line);
                let msg = msg.trim_end_matches(['\r', '\n']);
                if msg.is_empty() {
                    line.clear();
                    continue;
                }
                let now = now_iso();
                let entry = file_tail_line_to_entry(source, msg, &now);
                ingest.send_durable(entry).await?;
                registry.update_checkpoint(&source.id, identity.dev, identity.ino, position, &now)?;
                line.clear();
                let mut status = status.lock();
                status.last_line_at = Some(now);
                status.last_error = if read.truncated {
                    Some(format!(
                        "truncated oversized line from {} to {max_line_bytes} bytes",
                        source.path
                    ))
                } else {
                    None
                };
            }
        }
    }
}

struct PartialLineBeforeReopen<'a> {
    source: &'a FileTailSource,
    registry: &'a FileTailRegistry,
    ingest: &'a IngestTx,
    status: &'a Mutex<FileTailStatus>,
    line: &'a [u8],
    identity: FileIdentity,
    position: u64,
    now: &'a str,
}

async fn ingest_partial_line_before_reopen(partial: PartialLineBeforeReopen<'_>) -> Result<()> {
    let msg = String::from_utf8_lossy(partial.line);
    let msg = msg.trim_end_matches(['\r', '\n']);
    if msg.is_empty() {
        return Ok(());
    }
    partial
        .ingest
        .send_durable(file_tail_line_to_entry(partial.source, msg, partial.now))
        .await?;
    partial.registry.update_checkpoint(
        &partial.source.id,
        partial.identity.dev,
        partial.identity.ino,
        partial.position,
        partial.now,
    )?;
    let mut status = partial.status.lock();
    status.last_line_at = Some(partial.now.to_string());
    status.last_error = Some(format!(
        "ingested unterminated partial line before rotation/truncation for {}",
        partial.source.path
    ));
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    pub(crate) dev: u64,
    pub(crate) ino: u64,
}

#[derive(Debug)]
pub(crate) struct OpenedTailFile {
    pub(crate) file: tokio::fs::File,
    pub(crate) identity: FileIdentity,
    pub(crate) position: u64,
}

pub(crate) struct BoundedLine {
    pub(crate) bytes_read: usize,
    pub(crate) truncated: bool,
    pub(crate) complete: bool,
}

pub(crate) async fn open_tail_file(
    source: &FileTailSource,
    first_open: bool,
) -> Result<OpenedTailFile> {
    let mut file = open_validated_tail_file(&source.path).await?;
    let metadata = file.metadata().await?;
    let identity = FileIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
    };
    let checkpoint_matches = source.checkpoint_dev == Some(identity.dev)
        && source.checkpoint_ino == Some(identity.ino)
        && source
            .checkpoint_offset
            .is_some_and(|offset| offset <= metadata.len());
    let position = if checkpoint_matches {
        source.checkpoint_offset.unwrap_or(0)
    } else if first_open && source.start_at_end {
        metadata.len()
    } else {
        0
    };
    file.seek(std::io::SeekFrom::Start(position)).await?;
    Ok(OpenedTailFile {
        file,
        identity,
        position,
    })
}

pub(crate) async fn reopen_if_rotated_or_truncated(
    source: &FileTailSource,
    identity: FileIdentity,
    position: u64,
) -> Result<Option<OpenedTailFile>> {
    let metadata = match tokio::fs::metadata(&source.path).await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            anyhow::bail!("file-tail source disappeared: {}", source.path);
        }
        Err(err) => return Err(err.into()),
    };
    let current = FileIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
    };
    if current != identity || metadata.len() < position {
        let mut file = open_validated_tail_file(&source.path).await?;
        let metadata = file.metadata().await?;
        file.seek(std::io::SeekFrom::Start(0)).await?;
        return Ok(Some(OpenedTailFile {
            file,
            identity: FileIdentity {
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
            position: 0,
        }));
    }
    Ok(None)
}

async fn open_validated_tail_file(path: &str) -> Result<tokio::fs::File> {
    validate_file_tail_path(path)?;
    let path = path.to_string();
    let std_file = tokio::task::spawn_blocking({
        let path = path.clone();
        move || {
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)
        }
    })
    .await??;
    let metadata = std_file.metadata()?;
    validate_opened_file_tail_path(&path, &metadata)?;
    Ok(tokio::fs::File::from_std(std_file))
}

pub(crate) async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    out: &mut Vec<u8>,
    max_line_bytes: usize,
) -> std::io::Result<BoundedLine> {
    let mut bytes_read = 0;
    let mut truncated = false;

    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(BoundedLine {
                bytes_read,
                truncated,
                complete: false,
            });
        }

        let newline_pos = available.iter().position(|byte| *byte == b'\n');
        let consume_len = newline_pos.map_or(available.len(), |pos| pos + 1);
        let remaining = max_line_bytes.saturating_sub(out.len());
        let copy_len = remaining.min(consume_len);
        out.extend_from_slice(&available[..copy_len]);
        if copy_len < consume_len {
            truncated = true;
        }
        reader.consume(consume_len);
        bytes_read += consume_len;

        if newline_pos.is_some() {
            return Ok(BoundedLine {
                bytes_read,
                truncated,
                complete: true,
            });
        }
    }
}

pub(crate) fn file_tail_line_to_entry(
    source: &FileTailSource,
    line: &str,
    now: &str,
) -> LogBatchEntry {
    let hostname = source.hostname.clone().unwrap_or_else(local_hostname);
    let source_hostname = source_identity_component(&hostname);
    let path_basename = std::path::Path::new(&source.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let metadata_json = bounded_metadata_json(serde_json::json!({
        "source_type": "file_tail",
        "source_kind": SourceKind::FileTail.as_str(),
        "file_tail_id": source.id,
        "tag": source.tag,
        "path_basename": path_basename,
    }));
    let mut entry = LogBatchEntry {
        timestamp: now.to_string(),
        hostname: hostname.clone(),
        facility: source.facility.clone(),
        severity: source.severity.clone(),
        app_name: Some(source.tag.clone()),
        process_id: None,
        message: line.to_string(),
        raw: line.to_string(),
        source_ip: format!("file-tail://{source_hostname}/{}", source.id),
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
    stamp_source_kind(&mut entry, SourceKind::FileTail);
    entry
}

#[cfg(test)]
pub(crate) async fn tail_file_once_for_test(
    source: FileTailSource,
    ingest: IngestTx,
) -> Result<()> {
    let file = tokio::fs::File::open(&source.path).await?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 0 {
        let msg = line.trim_end_matches(['\r', '\n']);
        if !msg.is_empty() {
            ingest
                .send(file_tail_line_to_entry(
                    &source,
                    msg,
                    "2026-06-11T20:01:00Z",
                ))
                .await?;
        }
        line.clear();
    }
    Ok(())
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn local_hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|host| !host.trim().is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

fn source_identity_component(hostname: &str) -> String {
    let normalized = hostname
        .trim()
        .to_ascii_lowercase()
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') {
                byte as char
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '-', '_'])
        .to_string()
        .chars()
        .take(255)
        .collect::<String>();
    if normalized.is_empty() {
        "localhost".to_string()
    } else {
        normalized
    }
}
