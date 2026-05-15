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
const MAX_WATCH_DIRS: usize = 8192;
const MAX_PENDING_FILES: usize = 4096;

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
enum WatchTarget {
    Directory(PathBuf),
    File { path: PathBuf, parent: PathBuf },
}

impl WatchTarget {
    fn root(&self) -> &Path {
        match self {
            Self::Directory(path) => path,
            Self::File { parent, .. } => parent,
        }
    }

    fn allowed_file(&self) -> Option<&Path> {
        match self {
            Self::Directory(_) => None,
            Self::File { path, .. } => Some(path),
        }
    }
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
    fn push(&mut self, path: PathBuf, now: Instant) -> bool {
        if let Some(entry) = self.files.get_mut(&path) {
            entry.last_seen = now;
            self.coalesced_events += 1;
            return true;
        }
        if self.files.len() >= MAX_PENDING_FILES {
            return false;
        }
        self.files.insert(
            path,
            PendingFile {
                last_seen: now,
                retries: 0,
                last_len: None,
                last_mtime: None,
                stable_since: None,
            },
        );
        true
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

    fn clear(&mut self) {
        self.files.clear();
    }

    fn stable(&mut self, path: &Path, now: Instant, settle: Duration) -> Result<PendingState> {
        let Some(entry) = self.files.get_mut(path) else {
            return Ok(PendingState::Terminal);
        };
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => return Ok(PendingState::Terminal),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(PendingState::Terminal);
            }
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            return Ok(PendingState::Terminal);
        }
        let len = metadata.len();
        let mtime = metadata.modified().ok();
        if entry.last_len == Some(len) && entry.last_mtime == mtime {
            let stable_since = *entry.stable_since.get_or_insert(now);
            return Ok(if now.duration_since(stable_since) >= settle {
                PendingState::Stable
            } else {
                PendingState::NotReady
            });
        }
        entry.last_len = Some(len);
        entry.last_mtime = mtime;
        entry.stable_since = Some(now);
        Ok(PendingState::NotReady)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingState {
    NotReady,
    Stable,
    Terminal,
}

pub async fn run(service: SyslogService, options: WatchOptions) -> Result<()> {
    let targets = watch_targets(&options)?;
    if targets.is_empty() {
        anyhow::bail!("no AI transcript roots exist to watch");
    }

    let overflow_rescan = Arc::new(AtomicBool::new(false));
    let prune_missing = Arc::new(AtomicBool::new(false));
    let (tx, mut rx) = mpsc::channel::<notify::Result<Event>>(WATCH_EVENT_BUFFER);
    let callback_rescan = Arc::clone(&overflow_rescan);
    let callback_prune_missing = Arc::clone(&prune_missing);
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            if tx.try_send(event).is_err() {
                callback_rescan.store(true, Ordering::Relaxed);
                callback_prune_missing.store(true, Ordering::Relaxed);
            }
        },
        Config::default().with_follow_symlinks(false),
    )?;

    let mut watched_dirs = BTreeSet::new();
    for target in &targets {
        watch_directory_tree(&mut watcher, target.root(), &mut watched_dirs)?;
    }
    if watched_dirs.is_empty() {
        anyhow::bail!("no accessible AI transcript directories exist to watch");
    }

    tracing::info!(targets = ?targets, watched_dirs = watched_dirs.len(), "AI transcript watcher started");
    if options.initial_scan {
        let _ = run_rescan(&service, &options, "initial").await;
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
                for dir in handle_event(event, &targets, &mut pending, &overflow_rescan, &prune_missing) {
                    watch_directory_tree(&mut watcher, &dir, &mut watched_dirs)?;
                }
            }
            _ = tick.tick() => {
                while prune_missing.swap(false, Ordering::Relaxed) {
                    if prune_missing_checkpoints(&service, options.json).await {
                        prune_missing.store(true, Ordering::Relaxed);
                    }
                }
                if overflow_rescan.swap(false, Ordering::Relaxed)
                    && run_rescan(&service, &options, "rescan").await == RescanStatus::Retry
                {
                    overflow_rescan.store(true, Ordering::Relaxed);
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

fn watch_targets(options: &WatchOptions) -> Result<Vec<WatchTarget>> {
    if let Some(path) = &options.path {
        let canonical = scanner::validate_transcript_scan_path(path)?;
        if canonical.is_file() {
            let parent = canonical.parent().map(Path::to_path_buf).ok_or_else(|| {
                anyhow::anyhow!("transcript file has no parent: {}", canonical.display())
            })?;
            return Ok(vec![WatchTarget::File {
                path: canonical,
                parent,
            }]);
        }
        return Ok(vec![WatchTarget::Directory(canonical)]);
    }

    scanner::default_transcript_roots()
        .into_iter()
        .filter(|path| path.exists())
        .map(|path| scanner::validate_transcript_scan_path(&path).map(WatchTarget::Directory))
        .collect()
}

fn watch_directory_tree(
    watcher: &mut RecommendedWatcher,
    root: &Path,
    watched_dirs: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let dirs = collect_watch_dirs(root)?;
    for dir in dirs {
        if watched_dirs.contains(&dir) {
            continue;
        }
        if watched_dirs.len() >= MAX_WATCH_DIRS {
            anyhow::bail!(
                "AI transcript watcher directory budget exceeded ({MAX_WATCH_DIRS}); use a narrower --path or raise system inotify limits"
            );
        }
        match watcher.watch(&dir, RecursiveMode::NonRecursive) {
            Ok(()) => {
                watched_dirs.insert(dir);
            }
            Err(error) => anyhow::bail!(
                "failed to watch AI transcript directory {}: {error}",
                dir.display()
            ),
        }
    }
    Ok(())
}

fn collect_watch_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    if root.is_file() {
        if let Some(parent) = root.parent() {
            collect_watch_dirs_inner(parent, &mut dirs, true)?;
        }
    } else {
        collect_watch_dirs_inner(root, &mut dirs, true)?;
    }
    Ok(dirs)
}

fn collect_watch_dirs_inner(path: &Path, dirs: &mut Vec<PathBuf>, is_root: bool) -> Result<()> {
    if dirs.len() >= MAX_WATCH_DIRS {
        anyhow::bail!(
            "AI transcript watcher directory budget exceeded ({MAX_WATCH_DIRS}) while scanning {}",
            path.display()
        );
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            if is_root && !is_transient_watch_error(&error) {
                anyhow::bail!(
                    "failed to inspect AI transcript watch path {}: {error}",
                    path.display()
                );
            }
            tracing::warn!(path = %path.display(), error = %error, "skipping unreadable AI transcript watch path");
            return Ok(());
        }
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }

    let read_dir = match std::fs::read_dir(path) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            if is_root && !is_transient_watch_error(&error) {
                anyhow::bail!(
                    "failed to read AI transcript watch directory {}: {error}",
                    path.display()
                );
            }
            tracing::warn!(path = %path.display(), error = %error, "skipping unreadable AI transcript watch directory");
            return Ok(());
        }
    };
    dirs.push(path.to_path_buf());
    let mut entries = Vec::new();
    for entry in read_dir {
        match entry {
            Ok(entry) => entries.push(entry.path()),
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "skipping unreadable AI transcript watch directory entry");
            }
        }
    }
    entries.sort();
    for entry in entries {
        collect_watch_dirs_inner(&entry, dirs, false)?;
    }
    Ok(())
}

fn handle_event(
    event: notify::Result<Event>,
    targets: &[WatchTarget],
    pending: &mut PendingFiles,
    overflow_rescan: &AtomicBool,
    prune_missing: &AtomicBool,
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
                    if event.kind.is_create()
                        && path.is_dir()
                        && targets.iter().all(|target| target.allowed_file().is_none())
                    {
                        new_dirs.push(path);
                    } else if scanner::is_supported_transcript_file(&path)
                        && event_path_allowed(&path, targets)
                        && !pending.push(path, now)
                    {
                        tracing::warn!(
                            pending_files = pending.files.len(),
                            limit = MAX_PENDING_FILES,
                            "AI transcript pending queue exceeded; coalescing to root rescan"
                        );
                        pending.clear();
                        overflow_rescan.store(true, Ordering::Relaxed);
                    }
                }
            } else if event.kind.is_remove() {
                for path in event.paths {
                    if scanner::is_supported_transcript_file(&path)
                        && event_path_allowed_missing_ok(&path, targets)
                    {
                        pending.remove(&path);
                        prune_missing.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
        Err(error) => tracing::warn!(error = %error, "AI transcript watch event failed"),
    }
    new_dirs
}

fn is_transient_watch_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound
            | std::io::ErrorKind::PermissionDenied
            | std::io::ErrorKind::Other
    )
}

async fn prune_missing_checkpoints(service: &SyslogService, json: bool) -> bool {
    const PRUNE_LIMIT: u32 = 500;
    match service
        .prune_ai_checkpoints(true, false, Some(PRUNE_LIMIT))
        .await
    {
        Ok(result) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "stage": "prune_missing",
                        "result": result,
                    })
                );
            }
            tracing::info!(
                matched = result.matched,
                pruned = result.pruned,
                "AI transcript watcher pruned missing transcript checkpoints"
            );
            result.matched >= PRUNE_LIMIT as usize
        }
        Err(error) => {
            tracing::warn!(error = %error, "AI transcript watcher failed to prune missing checkpoints");
            false
        }
    }
}

fn event_path_allowed(path: &Path, targets: &[WatchTarget]) -> bool {
    let canonical = path.canonicalize().unwrap_or_else(|error| {
        tracing::warn!(
            path = %path.display(),
            error = %error,
            "AI transcript event path canonicalization failed; using original path"
        );
        path.to_path_buf()
    });
    canonical_path_allowed(&canonical, targets)
}

fn event_path_allowed_missing_ok(path: &Path, targets: &[WatchTarget]) -> bool {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canonical_path_allowed(&canonical, targets)
}

fn canonical_path_allowed(canonical: &Path, targets: &[WatchTarget]) -> bool {
    targets.iter().any(|target| match target {
        WatchTarget::Directory(root) => canonical.starts_with(root),
        WatchTarget::File { path, .. } => canonical == path,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RescanStatus {
    Completed,
    Retry,
}

async fn run_rescan(service: &SyslogService, options: &WatchOptions, stage: &str) -> RescanStatus {
    let path = options.path.as_ref().map(|path| path.display().to_string());
    match service.index_ai_roots(path, false, None).await {
        Ok(result) => {
            emit_index_result(stage, &result, options.json);
            RescanStatus::Completed
        }
        Err(error) => {
            tracing::warn!(error = %error, "AI transcript rescan failed");
            RescanStatus::Retry
        }
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
            Ok(PendingState::Stable) => {
                let outcome = process_file(service, options, &path).await;
                if outcome.retry {
                    if !pending.requeue(path.clone(), Instant::now(), options.max_retries) {
                        tracing::warn!(
                            path = %path.display(),
                            detail = outcome.detail.as_deref().unwrap_or("unknown failure"),
                            "AI transcript indexing failed before retry cap"
                        );
                        pending.remove(&path);
                    }
                } else {
                    pending.remove(&path);
                }
            }
            Ok(PendingState::NotReady) => {
                tracing::trace!(path = %path.display(), "AI transcript file is not stable yet");
            }
            Ok(PendingState::Terminal) => {
                pending.remove(&path);
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

struct ProcessOutcome {
    retry: bool,
    detail: Option<String>,
}

async fn process_file(
    service: &SyslogService,
    options: &WatchOptions,
    path: &Path,
) -> ProcessOutcome {
    tracing::debug!(path = %path.display(), "AI transcript watch indexing file");
    match service.add_ai_file(path.display().to_string(), false).await {
        Ok(result) => {
            emit_index_result("file", &result, options.json);
            let retry = result.parse_errors > 0
                || result.storage_blocked_chunks > 0
                || result.dropped_metadata_fields > 0
                || !result.file_errors.is_empty();
            let detail = retry.then(|| {
                format!(
                    "parse_errors={} storage_blocked_chunks={} dropped_metadata_fields={} file_errors={}",
                    result.parse_errors,
                    result.storage_blocked_chunks,
                    result.dropped_metadata_fields,
                    result.file_errors.len()
                )
            });
            ProcessOutcome { retry, detail }
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), error = %error, "AI transcript indexing failed");
            ProcessOutcome {
                retry: true,
                detail: Some(error.to_string()),
            }
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
        || result.dropped_metadata_fields > 0
        || !result.file_errors.is_empty()
    {
        println!(
            "{stage}: files={} ingested={} duplicates={} parse_errors={} storage_blocked={} dropped_metadata_fields={} file_errors={}",
            result.discovered_files,
            result.ingested,
            result.skipped_dupes,
            result.parse_errors,
            result.storage_blocked_chunks,
            result.dropped_metadata_fields,
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
#[path = "ai_watch_tests.rs"]
mod tests;
