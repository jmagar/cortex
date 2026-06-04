use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use notify::{
    Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::db::DbPool;

use super::background_interval;

/// Default cadence for refreshing the private homelab inventory cache consumed
/// by `cortex map`. Set `CORTEX_INVENTORY_REFRESH_INTERVAL_SECS=0` to disable.
const INVENTORY_REFRESH_INTERVAL_SECS: u64 = 300;
const INVENTORY_WATCH_DEBOUNCE_SECS: u64 = 3;
const REMOTE_DOCKER_EVENT_RECONNECT_SECS: u64 = 10;

pub fn spawn(
    token: CancellationToken,
    pool: Arc<DbPool>,
    maintenance_limiter: Arc<Semaphore>,
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
        let _docker_event_tasks =
            spawn_remote_docker_event_tasks(&watch_config, watch_tx, token.clone());
        let mut interval = background_interval(tokio::time::Duration::from_secs(interval_secs));
        let mut eager = true;
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
                    }
                    _ = interval.tick() => {}
                }
            }
            refresh_and_project(&pool, &maintenance_limiter).await;
        }
    }))
}

async fn refresh_and_project(pool: &DbPool, maintenance_limiter: &Arc<Semaphore>) {
    let started = Instant::now();
    let config = crate::inventory::InventoryConfig::from_env();
    match crate::inventory::refresh_inventory(config.clone()).await {
        Ok(report) => {
            let Ok(_permit) = Arc::clone(maintenance_limiter).acquire_owned().await else {
                tracing::error!("inventory_refresh: maintenance limiter closed");
                return;
            };
            let pool = pool.clone();
            let projection_pool = pool.clone();
            let projection = tokio::task::spawn_blocking(move || {
                let inventory = crate::inventory::read_inventory_cache(&config)
                    .context("read inventory cache for graph projection")?;
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
) -> Vec<JoinHandle<()>> {
    if !remote_docker_events_enabled() {
        tracing::info!("inventory_refresh: remote Docker event streams disabled");
        return Vec::new();
    }
    let hosts =
        crate::inventory::ssh::configured_hosts(config.ssh_config.as_deref(), &config.ssh_hosts);
    hosts
        .into_iter()
        .map(|host| {
            let ssh_config = config.ssh_config.clone();
            let trigger = trigger.clone();
            let token = token.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        biased;
                        _ = token.cancelled() => break,
                        result = run_remote_docker_events_once(&host, ssh_config.as_deref(), trigger.clone(), token.clone()) => {
                            if let Err(error) = result {
                                tracing::debug!(%error, host = %host, "inventory_refresh: remote Docker events unavailable");
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

async fn run_remote_docker_events_once(
    host: &str,
    ssh_config: Option<&Path>,
    trigger: mpsc::Sender<()>,
    token: CancellationToken,
) -> anyhow::Result<()> {
    let args = remote_docker_events_ssh_args(ssh_config, host);
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
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let _ = io::copy(&mut reader, &mut io::sink()).await;
    });
    let mut lines = BufReader::new(stdout).lines();
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
                    let _ = trigger.try_send(());
                }
                Some(_) => {}
                None => break,
            }
        }
    }
    let status = child.wait().await?;
    let _ = stderr_task.await;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("ssh docker events exited with {status}"))
    }
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

fn remote_docker_events_ssh_args(ssh_config: Option<&Path>, host: &str) -> Vec<String> {
    let mut args = Vec::new();
    args.push("-o".to_string());
    args.push("IgnoreUnknown=WarnWeakCrypto".to_string());
    if let Some(config) = ssh_config {
        args.push("-F".to_string());
        args.push(config.display().to_string());
    }
    args.extend([
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "ConnectTimeout=4".to_string(),
        "-o".to_string(),
        "ServerAliveInterval=15".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=2".to_string(),
        "--".to_string(),
        host.to_string(),
        "docker events --filter type=container --format '{{json .}}'".to_string(),
    ]);
    args
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
