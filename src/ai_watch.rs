use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::app::SyslogService;
use crate::scanner::{self, IndexResult};

const WATCH_EVENT_BUFFER: usize = 1024;

#[derive(Debug, Clone)]
pub struct WatchOptions {
    pub path: Option<PathBuf>,
    pub debounce: Duration,
    pub settle: Duration,
    pub max_retries: u8,
    pub initial_scan: bool,
    pub json: bool,
}

#[derive(Debug, Clone)]
struct PendingFile {
    last_seen: Instant,
    retries: u8,
    last_len: Option<u64>,
    last_mtime: Option<SystemTime>,
    stable_since: Option<Instant>,
}

#[derive(Debug, Default)]
struct PendingFiles {
    files: BTreeMap<PathBuf, PendingFile>,
    coalesced_events: u64,
}

impl PendingFiles {
    fn push(&mut self, path: PathBuf, now: Instant) {
        self.files
            .entry(path)
            .and_modify(|entry| {
                entry.last_seen = now;
                self.coalesced_events += 1;
            })
            .or_insert(PendingFile {
                last_seen: now,
                retries: 0,
                last_len: None,
                last_mtime: None,
                stable_since: None,
            });
    }

    fn requeue(&mut self, path: PathBuf, now: Instant, max_retries: u8) -> bool {
        let entry = self.files.entry(path).or_insert(PendingFile {
            last_seen: now,
            retries: 0,
            last_len: None,
            last_mtime: None,
            stable_since: None,
        });
        if entry.retries >= max_retries {
            return false;
        }
        entry.retries += 1;
        entry.last_seen = now;
        entry.stable_since = None;
        true
    }

    fn debounced_paths(&self, now: Instant, debounce: Duration) -> Vec<PathBuf> {
        self.files
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.last_seen) >= debounce)
            .map(|(path, _)| path.clone())
            .collect()
    }

    fn remove(&mut self, path: &Path) {
        self.files.remove(path);
    }

    fn stable(&mut self, path: &Path, now: Instant, settle: Duration) -> Result<bool> {
        let Some(entry) = self.files.get_mut(path) else {
            return Ok(false);
        };
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            return Ok(false);
        }
        let len = metadata.len();
        let mtime = metadata.modified().ok();
        if entry.last_len == Some(len) && entry.last_mtime == mtime {
            let stable_since = *entry.stable_since.get_or_insert(now);
            return Ok(now.duration_since(stable_since) >= settle);
        }
        entry.last_len = Some(len);
        entry.last_mtime = mtime;
        entry.stable_since = Some(now);
        Ok(false)
    }
}

pub async fn run(service: SyslogService, options: WatchOptions) -> Result<()> {
    let roots = watch_roots(&options);
    if roots.is_empty() {
        anyhow::bail!("no AI transcript roots exist to watch");
    }

    let overflow_rescan = Arc::new(AtomicBool::new(false));
    let (tx, mut rx) = mpsc::channel::<notify::Result<Event>>(WATCH_EVENT_BUFFER);
    let callback_rescan = Arc::clone(&overflow_rescan);
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            if tx.try_send(event).is_err() {
                callback_rescan.store(true, Ordering::Relaxed);
            }
        },
        Config::default().with_follow_symlinks(false),
    )?;

    let mut watched_dirs = BTreeSet::new();
    for root in &roots {
        watch_directory_tree(&mut watcher, root, &mut watched_dirs);
    }
    if watched_dirs.is_empty() {
        anyhow::bail!("no accessible AI transcript directories exist to watch");
    }

    tracing::info!(roots = ?roots, watched_dirs = watched_dirs.len(), "AI transcript watcher started");
    if options.initial_scan {
        run_rescan(&service, &options, "initial").await;
    }

    let tick_duration = options
        .debounce
        .min(options.settle)
        .max(Duration::from_millis(50));
    let mut tick = tokio::time::interval(tick_duration);
    let mut pending = PendingFiles::default();

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                for dir in handle_event(event, &mut pending, &overflow_rescan) {
                    watch_directory_tree(&mut watcher, &dir, &mut watched_dirs);
                }
            }
            _ = tick.tick() => {
                if overflow_rescan.swap(false, Ordering::Relaxed) {
                    run_rescan(&service, &options, "rescan").await;
                }
                process_pending(&service, &options, &mut pending).await;
            }
            _ = shutdown_signal() => {
                tracing::info!("AI transcript watcher stopping");
                return Ok(());
            }
        }
    }
}

fn watch_roots(options: &WatchOptions) -> Vec<PathBuf> {
    let roots = match &options.path {
        Some(path) => vec![path.clone()],
        None => scanner::default_transcript_roots(),
    };
    roots.into_iter().filter(|path| path.exists()).collect()
}

fn watch_directory_tree(
    watcher: &mut RecommendedWatcher,
    root: &Path,
    watched_dirs: &mut BTreeSet<PathBuf>,
) {
    let dirs = collect_watch_dirs(root);
    for dir in dirs {
        if watched_dirs.contains(&dir) {
            continue;
        }
        match watcher.watch(&dir, RecursiveMode::NonRecursive) {
            Ok(()) => {
                watched_dirs.insert(dir);
            }
            Err(error) => {
                tracing::warn!(path = %dir.display(), error = %error, "failed to watch AI transcript directory");
            }
        }
    }
}

fn collect_watch_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if root.is_file() {
        if let Some(parent) = root.parent() {
            collect_watch_dirs_inner(parent, &mut dirs);
        }
    } else {
        collect_watch_dirs_inner(root, &mut dirs);
    }
    dirs
}

fn collect_watch_dirs_inner(path: &Path, dirs: &mut Vec<PathBuf>) {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            tracing::warn!(path = %path.display(), error = %error, "failed to inspect AI transcript watch path");
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_file() {
        return;
    }
    if !metadata.is_dir() {
        return;
    }

    dirs.push(path.to_path_buf());
    let read_dir = match std::fs::read_dir(path) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            tracing::warn!(path = %path.display(), error = %error, "failed to read AI transcript watch directory");
            return;
        }
    };
    let mut entries = Vec::new();
    for entry in read_dir {
        match entry {
            Ok(entry) => entries.push(entry.path()),
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "failed to read AI transcript watch directory entry");
            }
        }
    }
    entries.sort();
    for entry in entries {
        collect_watch_dirs_inner(&entry, dirs);
    }
}

fn handle_event(
    event: notify::Result<Event>,
    pending: &mut PendingFiles,
    overflow_rescan: &AtomicBool,
) -> Vec<PathBuf> {
    let mut new_dirs = Vec::new();
    match event {
        Ok(event) => {
            tracing::debug!(kind = ?event.kind, paths = ?event.paths, "AI transcript watch event received");
            if event.need_rescan() {
                overflow_rescan.store(true, Ordering::Relaxed);
                return new_dirs;
            }
            if event.kind.is_create() || event.kind.is_modify() {
                let now = Instant::now();
                for path in event.paths {
                    if event.kind.is_create() && path.is_dir() {
                        new_dirs.push(path);
                    } else if scanner::is_supported_transcript_file(&path) {
                        pending.push(path, now);
                    }
                }
            }
        }
        Err(error) => tracing::warn!(error = %error, "AI transcript watch event failed"),
    }
    new_dirs
}

async fn run_rescan(service: &SyslogService, options: &WatchOptions, stage: &str) {
    let path = options.path.as_ref().map(|path| path.display().to_string());
    match service.index_ai_roots(path, false, None).await {
        Ok(result) => emit_index_result(stage, &result, options.json),
        Err(error) => tracing::warn!(error = %error, "AI transcript rescan failed"),
    }
}

async fn process_pending(
    service: &SyslogService,
    options: &WatchOptions,
    pending: &mut PendingFiles,
) {
    let now = Instant::now();
    let paths = pending.debounced_paths(now, options.debounce);
    for path in paths {
        match pending.stable(&path, now, options.settle) {
            Ok(true) => {
                if process_file(service, options, &path).await {
                    if !pending.requeue(path.clone(), Instant::now(), options.max_retries) {
                        tracing::warn!(path = %path.display(), "AI transcript indexing failed before retry cap");
                        pending.remove(&path);
                    }
                } else {
                    pending.remove(&path);
                }
            }
            Ok(false) => {
                tracing::trace!(path = %path.display(), "AI transcript file is not stable yet");
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "AI transcript metadata check failed");
                if !pending.requeue(path.clone(), now, options.max_retries) {
                    pending.remove(&path);
                }
            }
        }
    }
}

async fn process_file(service: &SyslogService, options: &WatchOptions, path: &Path) -> bool {
    tracing::debug!(path = %path.display(), "AI transcript watch indexing file");
    match service.add_ai_file(path.display().to_string(), false).await {
        Ok(result) => {
            emit_index_result("file", &result, options.json);
            result.parse_errors > 0
                || result.storage_blocked_chunks > 0
                || !result.file_errors.is_empty()
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), error = %error, "AI transcript indexing failed");
            true
        }
    }
}

fn emit_index_result(stage: &str, result: &IndexResult, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "stage": stage,
                "result": result,
            })
        );
        return;
    }
    if result.ingested > 0
        || result.parse_errors > 0
        || result.storage_blocked_chunks > 0
        || !result.file_errors.is_empty()
    {
        println!(
            "{stage}: files={} ingested={} duplicates={} parse_errors={} storage_blocked={} file_errors={}",
            result.discovered_files,
            result.ingested,
            result.skipped_dupes,
            result.parse_errors,
            result.storage_blocked_chunks,
            result.file_errors.len()
        );
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_files_deduplicate_and_requeue_with_cap() {
        let start = Instant::now();
        let path = PathBuf::from("/tmp/session.jsonl");
        let mut pending = PendingFiles::default();
        pending.push(path.clone(), start);
        pending.push(path.clone(), start + Duration::from_millis(25));

        assert_eq!(pending.files.len(), 1);
        assert!(pending
            .debounced_paths(
                start + Duration::from_millis(100),
                Duration::from_millis(200)
            )
            .is_empty());
        assert_eq!(
            pending.debounced_paths(
                start + Duration::from_millis(300),
                Duration::from_millis(200)
            ),
            vec![path.clone()]
        );
        assert!(pending.requeue(path.clone(), start + Duration::from_millis(301), 1));
        assert!(!pending.requeue(path, start + Duration::from_millis(302), 1));
    }

    #[test]
    fn pending_files_wait_until_file_is_stable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.jsonl");
        std::fs::write(&path, "{}\n").unwrap();

        let start = Instant::now();
        let mut pending = PendingFiles::default();
        pending.push(path.clone(), start);

        assert!(!pending
            .stable(&path, start, Duration::from_millis(100))
            .unwrap());
        assert!(!pending
            .stable(
                &path,
                start + Duration::from_millis(50),
                Duration::from_millis(100)
            )
            .unwrap());
        assert!(pending
            .stable(
                &path,
                start + Duration::from_millis(150),
                Duration::from_millis(100)
            )
            .unwrap());
        assert_eq!(pending.files.get(&path).unwrap().retries, 0);
    }

    #[test]
    fn collect_watch_dirs_includes_accessible_directories_without_file_recursion() {
        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join("project");
        std::fs::create_dir(&nested).unwrap();
        let file = nested.join("session.jsonl");
        std::fs::write(&file, "{}\n").unwrap();

        let dirs = collect_watch_dirs(temp.path());

        assert!(dirs.contains(&temp.path().to_path_buf()));
        assert!(dirs.contains(&nested));
        assert!(!dirs.contains(&file));
    }
}
