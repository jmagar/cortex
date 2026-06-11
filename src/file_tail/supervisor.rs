use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::db::LogBatchEntry;
use crate::enrich::{SourceKind, stamp_source_kind};
use crate::ingest::IngestTx;
use crate::ingest_metadata::bounded_metadata_json;

use super::models::{FileTailSource, FileTailStatus};
use super::registry::FileTailRegistry;

#[derive(Clone)]
pub(crate) struct FileTailSupervisor {
    registry: Arc<FileTailRegistry>,
    ingest: IngestTx,
    token: CancellationToken,
    tasks: Arc<Mutex<HashMap<String, TailTask>>>,
}

struct TailTask {
    handle: JoinHandle<()>,
    status: Arc<Mutex<FileTailStatus>>,
}

impl FileTailSupervisor {
    pub(crate) fn new(
        registry: Arc<FileTailRegistry>,
        ingest: IngestTx,
        token: CancellationToken,
    ) -> Self {
        Self {
            registry,
            ingest,
            token,
            tasks: Arc::new(Mutex::new(HashMap::new())),
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

    pub(crate) fn reconcile(&self) -> Result<()> {
        let sources = self.registry.list()?;
        let enabled: HashSet<String> = sources
            .iter()
            .filter(|source| source.enabled)
            .map(|source| source.id.clone())
            .collect();

        {
            let mut tasks = self.tasks.lock();
            tasks.retain(|id, task| {
                if enabled.contains(id) {
                    true
                } else {
                    task.status.lock().running = false;
                    task.handle.abort();
                    false
                }
            });
        }

        for source in sources.into_iter().filter(|source| source.enabled) {
            if self.tasks.lock().contains_key(&source.id) {
                continue;
            }
            self.spawn_source(source);
        }
        Ok(())
    }

    fn spawn_source(&self, source: FileTailSource) {
        let id = source.id.clone();
        let status = Arc::new(Mutex::new(FileTailStatus {
            id: id.clone(),
            running: true,
            last_line_at: None,
            last_error: None,
        }));
        let task_status = Arc::clone(&status);
        let ingest = self.ingest.clone();
        let token = self.token.clone();
        let handle = tokio::spawn(async move {
            tail_file_loop(source, ingest, token, task_status).await;
        });
        self.tasks.lock().insert(id, TailTask { handle, status });
    }
}

async fn tail_file_loop(
    source: FileTailSource,
    ingest: IngestTx,
    token: CancellationToken,
    status: Arc<Mutex<FileTailStatus>>,
) {
    loop {
        if token.is_cancelled() {
            status.lock().running = false;
            return;
        }
        match tail_file_until_cancelled(&source, ingest.clone(), token.clone(), Arc::clone(&status))
            .await
        {
            Ok(()) => {
                status.lock().running = false;
                return;
            }
            Err(err) => {
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
    ingest: IngestTx,
    token: CancellationToken,
    status: Arc<Mutex<FileTailStatus>>,
) -> Result<()> {
    let mut file = tokio::fs::File::open(&source.path)
        .await
        .with_context(|| format!("open {}", source.path))?;
    if source.start_at_end {
        file.seek(std::io::SeekFrom::End(0)).await?;
    }
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        tokio::select! {
            _ = token.cancelled() => return Ok(()),
            read = reader.read_line(&mut line) => {
                let bytes = read?;
                if bytes == 0 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                let msg = line.trim_end_matches(['\r', '\n']);
                if msg.is_empty() {
                    continue;
                }
                let now = now_iso();
                let entry = file_tail_line_to_entry(source, msg, &now);
                ingest.send(entry).await?;
                let mut status = status.lock();
                status.last_line_at = Some(now);
                status.last_error = None;
            }
        }
    }
}

pub(crate) fn file_tail_line_to_entry(
    source: &FileTailSource,
    line: &str,
    now: &str,
) -> LogBatchEntry {
    let hostname = source.hostname.clone().unwrap_or_else(local_hostname);
    let metadata_json = bounded_metadata_json(serde_json::json!({
        "source_type": "file_tail",
        "source_kind": SourceKind::FileTail.as_str(),
        "file_tail_id": source.id,
        "path": source.path,
        "tag": source.tag,
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
        source_ip: format!("file-tail://{hostname}/{}", source.id),
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
