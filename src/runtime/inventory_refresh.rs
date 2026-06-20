use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{
    Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::db::DbPool;
use crate::observability::RuntimeObservability;

use super::background_interval;

/// Default cadence for refreshing the private homelab inventory cache consumed
/// by `cortex map`. Set `CORTEX_INVENTORY_REFRESH_INTERVAL_SECS=0` to disable.
const INVENTORY_REFRESH_INTERVAL_SECS: u64 = 300;
const INVENTORY_WATCH_DEBOUNCE_SECS: u64 = 3;
/// Minimum interval between watch-triggered refreshes. A crash-looping
/// container on any monitored host emits `docker events` lines continuously;
/// with only the 3s debounce each burst re-triggered a full SSH fan-out plus
/// a graph projection under the write lock, indefinitely (full-review PM5).
/// Watch events arriving inside the cooldown coalesce into one trailing
/// refresh; the 5-minute interval tick still guarantees eventual consistency.
const INVENTORY_WATCH_COOLDOWN_SECS: u64 = 60;
const REMOTE_DOCKER_EVENT_RECONNECT_SECS: u64 = 10;
const REMOTE_DOCKER_EVENTS_UNSUPPORTED_MARKER: &str =
    "cortex: remote Docker events unsupported: docker command not found";

pub fn spawn(
    token: CancellationToken,
    pool: Arc<DbPool>,
    maintenance_limiter: Arc<Semaphore>,
    observability: Arc<RuntimeObservability>,
) -> Option<JoinHandle<()>> {
    let interval_secs = inventory_refresh_interval_secs();
    if interval_secs == 0 {
        tracing::info!("inventory_refresh: disabled");
        return None;
    }
    Some(tokio::spawn(async move {
        let watch_config = crate::inventory::InventoryConfig::from_env();
        let (watch_tx, mut watch_rx) = mpsc::channel(64);
        let _watcher = start_config_watcher(&watch_config, watch_tx.clone());
        let _docker_event_tasks = spawn_remote_docker_event_tasks(
            &watch_config,
            watch_tx,
            token.clone(),
            Arc::clone(&observability),
        );
        let mut interval = background_interval(tokio::time::Duration::from_secs(interval_secs));
        let mut eager = true;
        let mut last_refresh: Option<Instant> = None;
        loop {
            if eager {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        tracing::debug!("inventory_refresh: cancelled before first refresh");
                        break;
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(15)) => {}
                }
                eager = false;
            } else {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        tracing::debug!("inventory_refresh: cooperative shutdown");
                        break;
                    }
                    Some(()) = watch_rx.recv() => {
                        debounce_watch_events(&mut watch_rx).await;
                        // Trailing-edge cooldown: wait out the remainder of
                        // INVENTORY_WATCH_COOLDOWN_SECS since the last refresh,
                        // coalescing any further watch events that arrive in
                        // the meantime (full-review PM5).
                        if let Some(last) = last_refresh {
                            let cooldown =
                                Duration::from_secs(INVENTORY_WATCH_COOLDOWN_SECS);
                            let since = last.elapsed();
                            if since < cooldown {
                                tokio::select! {
                                    biased;
                                    _ = token.cancelled() => {
                                        tracing::debug!(
                                            "inventory_refresh: cooperative shutdown"
                                        );
                                        break;
                                    }
                                    _ = tokio::time::sleep(cooldown - since) => {}
                                }
                                while watch_rx.try_recv().is_ok() {}
                            }
                        }
                    }
                    _ = interval.tick() => {}
                }
            }
            observability.record_task_tick("inventory_refresh");
            refresh_and_project(&pool, &maintenance_limiter).await;
            last_refresh = Some(Instant::now());
        }
    }))
}

async fn refresh_and_project(pool: &DbPool, maintenance_limiter: &Arc<Semaphore>) {
    let started = Instant::now();
    let config = crate::inventory::InventoryConfig::from_env();
    match crate::inventory::refresh_inventory_with_inventory(config).await {
        Ok(outcome) => {
            let report = outcome.report;
            let inventory = outcome.inventory;
            let Ok(_permit) = Arc::clone(maintenance_limiter).acquire_owned().await else {
                tracing::error!("inventory_refresh: maintenance limiter closed");
                return;
            };
            let pool = pool.clone();
            let projection_pool = pool.clone();
            let projection = tokio::task::spawn_blocking(move || {
                crate::db::graph_inventory::project_inventory(&pool, &inventory)
            })
            .await
            .unwrap_or_else(|error| {
                Err(anyhow::Error::new(error).context("join graph projection task"))
            });
            match projection {
                Ok(stats) => tracing::info!(
                    status = %report.status,
                    run_id = %report.run_id,
                    warnings = report.warnings.len(),
                    graph_entities = stats.entity_count,
                    graph_relationships = stats.relationship_count,
                    graph_evidence = stats.evidence_count,
                    elapsed_ms = started.elapsed().as_millis(),
                    "inventory_refresh: cache refresh and graph projection complete"
                ),
                Err(error) => {
                    let projection_error = error.to_string();
                    let mark_result = tokio::task::spawn_blocking(move || {
                        crate::db::graph_inventory::mark_inventory_projection_failed(
                            &projection_pool,
                            &projection_error,
                        )
                    })
                    .await
                    .unwrap_or_else(|join_error| {
                        Err(anyhow::Error::new(join_error)
                            .context("join projection failure marker task"))
                    });
                    if let Err(mark_error) = mark_result {
                        tracing::error!(
                            projection_error = %error,
                            mark_error = %mark_error,
                            "inventory_refresh: failed to persist projection degraded state"
                        );
                    }
                    tracing::warn!(
                        %error,
                        status = %report.status,
                        run_id = %report.run_id,
                        warnings = report.warnings.len(),
                        elapsed_ms = started.elapsed().as_millis(),
                        "inventory_refresh: cache refresh complete but graph projection failed"
                    );
                }
            }
        }
        Err(error) => tracing::warn!(
            %error,
            elapsed_ms = started.elapsed().as_millis(),
            "inventory_refresh: cache refresh failed"
        ),
    }
}

fn start_config_watcher(
    config: &crate::inventory::InventoryConfig,
    trigger: mpsc::Sender<()>,
) -> Option<RecommendedWatcher> {
    if !inventory_watch_enabled() {
        tracing::info!("inventory_refresh: local config watcher disabled");
        return None;
    }
    let targets = watched_config_targets(config);
    if targets.is_empty() {
        tracing::debug!("inventory_refresh: no local config paths to watch");
        return None;
    }
    let watch_dirs = watch_directories(&targets);
    let callback_targets = targets.clone();
    let mut watcher = match RecommendedWatcher::new(
        move |event| {
            if should_refresh_for_event(&event, &callback_targets) {
                let _ = trigger.try_send(());
            }
        },
        NotifyConfig::default(),
    ) {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!(%error, "inventory_refresh: failed to create config watcher");
            return None;
        }
    };
    let mut watched = 0usize;
    for dir in watch_dirs {
        match watcher.watch(&dir, RecursiveMode::NonRecursive) {
            Ok(()) => watched += 1,
            Err(error) => tracing::debug!(
                %error,
                path = %dir.display(),
                "inventory_refresh: failed to watch config path"
            ),
        }
    }
    if watched == 0 {
        return None;
    }
    tracing::info!(watched, "inventory_refresh: local config watcher active");
    Some(watcher)
}

async fn debounce_watch_events(rx: &mut mpsc::Receiver<()>) {
    tokio::time::sleep(tokio::time::Duration::from_secs(
        INVENTORY_WATCH_DEBOUNCE_SECS,
    ))
    .await;
    while rx.try_recv().is_ok() {}
}

fn spawn_remote_docker_event_tasks(
    config: &crate::inventory::InventoryConfig,
    trigger: mpsc::Sender<()>,
    token: CancellationToken,
    observability: Arc<RuntimeObservability>,
) -> Vec<JoinHandle<()>> {
    if !remote_docker_events_enabled() {
        tracing::info!("inventory_refresh: remote Docker event streams disabled");
        return Vec::new();
    }
    let resolution =
        crate::inventory::ssh::configured_hosts(config.ssh_config.as_deref(), &config.ssh_hosts);
    for warning in &resolution.warnings {
        tracing::warn!(warning = %warning, "inventory_refresh: remote Docker event host resolution degraded");
    }
    if resolution.no_usable_explicit_hosts() {
        tracing::warn!(
            "inventory_refresh: remote Docker event streams skipped; no explicitly configured SSH hosts were usable"
        );
        return Vec::new();
    }
    let ssh_context = crate::inventory::ssh::SshContext::new(
        crate::inventory::ssh::SshOptions::from_env(config.ssh_config.as_deref())
            .with_event_stream_defaults()
            .with_max_concurrent(resolution.hosts.len().max(1))
            .expect("event stream host count is non-zero after host resolution"),
    );
    resolution
        .hosts
        .into_iter()
        .map(|host| {
            let trigger = trigger.clone();
            let token = token.clone();
            let ssh_context = ssh_context.clone();
            let observability = Arc::clone(&observability);
            tokio::spawn(async move {
                let mut failure_log = EventStreamFailureLog::default();
                loop {
                    tokio::select! {
                        biased;
                        _ = token.cancelled() => break,
                        result = run_remote_docker_events_once(&host, &ssh_context, trigger.clone(), token.clone()) => {
                            if let Err(error) = result {
                                let error = error.to_string();
                                if remote_docker_events_unsupported(&error) {
                                    observability.record_remote_docker_event_stream_failure(&host, &error);
                                    tracing::warn!(
                                        host,
                                        error,
                                        "inventory_refresh: remote Docker event streams disabled for host; Docker is unavailable"
                                    );
                                    break;
                                }
                                observability.record_remote_docker_event_stream_failure(&host, &error);
                                failure_log.record(&host, &error);
                            }
                        }
                    }
                    tokio::select! {
                        biased;
                        _ = token.cancelled() => break,
                        _ = tokio::time::sleep(Duration::from_secs(REMOTE_DOCKER_EVENT_RECONNECT_SECS)) => {}
                    }
                }
            })
        })
        .collect()
}

#[derive(Default)]
struct EventStreamFailureLog {
    failures: u64,
}

impl EventStreamFailureLog {
    fn record(&mut self, host: &str, error: &str) {
        self.failures = self.failures.saturating_add(1);
        if self.failures == 1 || self.failures % 6 == 0 {
            tracing::warn!(
                host,
                failures = self.failures,
                error,
                "inventory_refresh: remote Docker event stream degraded"
            );
        } else {
            tracing::debug!(
                host,
                failures = self.failures,
                error,
                "inventory_refresh: remote Docker event stream still degraded"
            );
        }
    }
}

async fn run_remote_docker_events_once(
    host: &str,
    ssh_context: &crate::inventory::ssh::SshContext,
    trigger: mpsc::Sender<()>,
    token: CancellationToken,
) -> anyhow::Result<()> {
    let Some(_permit) = ssh_context.acquire_owned_cancellable(&token).await? else {
        return Ok(());
    };
    let args = remote_docker_events_ssh_args(ssh_context, host)?;
    let mut child = Command::new("ssh")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("ssh stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("ssh stderr unavailable"))?;
    let stderr_task = tokio::spawn(async move { read_stream_sample(stderr).await });
    let mut lines = BufReader::new(stdout).lines();
    let mut stdout_sample = OutputSample::default();
    loop {
        tokio::select! {
            biased;
            _ = token.cancelled() => {
                let _ = child.kill().await;
                stderr_task.abort();
                break;
            }
            line = lines.next_line() => match line? {
                Some(line) if !line.trim().is_empty() => {
                    stdout_sample.push_line(&line);
                    let _ = trigger.try_send(());
                }
                Some(_) => {}
                None => break,
            }
        }
    }
    let status = child.wait().await?;
    let stderr_sample = stderr_task
        .await
        .unwrap_or_else(|error| format!("stderr reader task failed: {error}"));
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "ssh docker events exited with status={status}; stdout_sample={}; stderr_sample={}",
            stdout_sample.as_str(),
            stderr_sample
        ))
    }
}

#[derive(Default)]
struct OutputSample {
    text: String,
    truncated: bool,
}

impl OutputSample {
    fn push_line(&mut self, line: &str) {
        const MAX_SAMPLE_BYTES: usize = 4096;
        if self.text.len() >= MAX_SAMPLE_BYTES {
            self.truncated = true;
            return;
        }
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        let remaining = MAX_SAMPLE_BYTES.saturating_sub(self.text.len());
        if line.len() > remaining {
            let safe_end = line
                .char_indices()
                .map(|(idx, _)| idx)
                .take_while(|idx| *idx <= remaining)
                .last()
                .unwrap_or(0);
            self.text.push_str(&line[..safe_end]);
            self.truncated = true;
        } else {
            self.text.push_str(line);
        }
    }

    fn as_str(&self) -> String {
        if self.truncated {
            format!("{}...<truncated>", self.text)
        } else {
            self.text.clone()
        }
    }
}

async fn read_stream_sample<R>(mut reader: R) -> String
where
    R: tokio::io::AsyncRead + Unpin,
{
    const MAX_SAMPLE_BYTES: usize = 4096;
    let mut sample = Vec::new();
    let mut buf = [0u8; 1024];
    let mut truncated = false;
    loop {
        match tokio::io::AsyncReadExt::read(&mut reader, &mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let remaining = MAX_SAMPLE_BYTES.saturating_sub(sample.len());
                if remaining == 0 {
                    truncated = true;
                    continue;
                }
                let take = n.min(remaining);
                sample.extend_from_slice(&buf[..take]);
                truncated |= take < n;
            }
            Err(error) => return format!("stderr read failed: {error}"),
        }
    }
    let mut text = String::from_utf8_lossy(&sample).to_string();
    if truncated {
        text.push_str("...<truncated>");
    }
    text
}

fn inventory_refresh_interval_secs() -> u64 {
    std::env::var("CORTEX_INVENTORY_REFRESH_INTERVAL_SECS")
        .ok()
        .as_deref()
        .and_then(parse_inventory_refresh_interval_secs)
        .unwrap_or(INVENTORY_REFRESH_INTERVAL_SECS)
}

fn parse_inventory_refresh_interval_secs(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok()
}

fn inventory_watch_enabled() -> bool {
    std::env::var("CORTEX_INVENTORY_WATCH_ENABLED")
        .ok()
        .as_deref()
        .map(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no"
            )
        })
        .unwrap_or(true)
}

fn remote_docker_events_enabled() -> bool {
    std::env::var("CORTEX_INVENTORY_REMOTE_DOCKER_EVENTS")
        .ok()
        .as_deref()
        .map(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no"
            )
        })
        .unwrap_or(false)
}

fn remote_docker_events_ssh_args(
    ssh_context: &crate::inventory::ssh::SshContext,
    host: &str,
) -> anyhow::Result<Vec<String>> {
    let command = format!(
        "if ! command -v docker >/dev/null 2>&1; then echo '{REMOTE_DOCKER_EVENTS_UNSUPPORTED_MARKER}' >&2; exit 78; fi; exec docker events --filter type=container --format '{{{{json .}}}}'"
    );
    ssh_context.ssh_args(host, &command)
}

fn remote_docker_events_unsupported(error: &str) -> bool {
    error.contains(REMOTE_DOCKER_EVENTS_UNSUPPORTED_MARKER)
}

fn watched_config_targets(config: &crate::inventory::InventoryConfig) -> Vec<PathBuf> {
    let mut paths = config.compose_paths.clone();
    paths.extend(config.proxy_paths.clone());
    paths.sort();
    paths.dedup();
    paths
}

fn watch_directories(targets: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = targets
        .iter()
        .filter_map(|path| {
            if path.is_dir() {
                Some(path.clone())
            } else {
                path.parent().map(Path::to_path_buf)
            }
        })
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.dedup();
    dirs
}

fn should_refresh_for_event(event: &notify::Result<Event>, targets: &[PathBuf]) -> bool {
    let Ok(event) = event else {
        return false;
    };
    if !matches!(
        event.kind,
        EventKind::Any | EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return false;
    }
    event.paths.iter().any(|changed| {
        targets
            .iter()
            .any(|target| path_matches_target(changed, target))
    })
}

fn path_matches_target(changed: &Path, target: &Path) -> bool {
    changed == target || target.is_dir() && changed.starts_with(target)
}

#[cfg(test)]
#[path = "inventory_refresh_tests.rs"]
mod tests;
