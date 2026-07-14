use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use bollard::Docker;
use bollard::query_parameters::{ListContainersOptionsBuilder, LogsOptionsBuilder};
use chrono::Utc;
use futures_util::StreamExt;
use tokio::task::JoinSet;
use tokio::time::sleep;

use super::syslog_sender::{PRI_LOCAL0_INFO, PRI_LOCAL0_WARN, SyslogSender, format_rfc5424};

const CONTAINER_POLL_SECS: u64 = 30;

/// Message prefix marker carrying structured agent Docker identity metadata.
/// The receiver enrichment path (`src/receiver/enrichment.rs`) extracts the
/// JSON payload into `metadata_json` and strips the marker from `message`.
/// Keep in sync with `AGENT_DOCKER_META_MARKER` there.
pub(crate) const AGENT_DOCKER_META_MARKER: &str = "[cortex-agent-docker-meta:";

struct ContainerInfo {
    id: String,
    name: String,
    app_name: String,
    image: Option<String>,
    labels: HashMap<String, String>,
}

/// Stream Docker container logs from a local socket and forward as RFC 5424
/// syslog to the given sender.  Runs until cancelled or a fatal error occurs.
pub async fn run_docker_forwarder(
    docker_url: &str,
    hostname: &str,
    sender: Arc<SyslogSender>,
) -> Result<()> {
    let docker = connect(docker_url).context("connect to Docker")?;
    docker.ping().await.context("Docker ping")?;
    tracing::info!(docker_url, "docker forwarder connected");

    let mut active: HashMap<String, tokio::task::AbortHandle> = HashMap::new();
    let mut tasks: JoinSet<String> = JoinSet::new(); // yields container_id on exit

    loop {
        let containers = list_containers(&docker).await?;
        let live_ids: std::collections::HashSet<String> =
            containers.iter().map(|c| c.id.clone()).collect();

        // Remove handles for containers that are no longer running.
        active.retain(|id, handle| {
            if !live_ids.contains(id) {
                handle.abort();
                false
            } else {
                true
            }
        });

        // Spawn a follower for any new container.
        for c in containers {
            if active.contains_key(&c.id) {
                continue;
            }
            let docker2 = docker.clone();
            let sender2 = Arc::clone(&sender);
            let hostname = hostname.to_string();
            let id = c.id.clone();
            let id2 = id.clone();
            let handle = tasks.spawn(async move {
                if let Err(e) = follow_container(&docker2, &hostname, &c, sender2).await {
                    tracing::debug!(
                        container = c.name,
                        error = %e,
                        "container log stream ended"
                    );
                }
                id
            });
            active.insert(id2, handle);
        }

        // Reap finished tasks.
        while let Ok(Some(res)) =
            tokio::time::timeout(Duration::from_millis(1), tasks.join_next()).await
        {
            if let Ok(id) = res {
                active.remove(&id);
            }
        }

        sleep(Duration::from_secs(CONTAINER_POLL_SECS)).await;
    }
}

async fn follow_container(
    docker: &Docker,
    hostname: &str,
    container: &ContainerInfo,
    sender: Arc<SyslogSender>,
) -> Result<()> {
    let since = Utc::now().timestamp() - 1;
    let opts = LogsOptionsBuilder::default()
        .stdout(true)
        .stderr(true)
        .timestamps(true)
        .follow(true)
        .since(since.clamp(0, i32::MAX as i64) as i32)
        .build();

    let mut stream = docker.logs(&container.id, Some(opts));
    while let Some(output) = stream.next().await {
        let (is_stderr, bytes) = match output? {
            bollard::container::LogOutput::StdOut { message } => (false, message),
            bollard::container::LogOutput::StdErr { message } => (true, message),
            _ => continue,
        };

        let raw = String::from_utf8_lossy(&bytes);
        let msg = raw.trim_end_matches(['\r', '\n']);
        if msg.is_empty() {
            continue;
        }
        let pri = if is_stderr {
            PRI_LOCAL0_WARN
        } else {
            PRI_LOCAL0_INFO
        };
        let stream = if is_stderr { "stderr" } else { "stdout" };
        let metadata = container_identity_metadata(
            hostname,
            &container.id,
            &container.name,
            stream,
            container.image.as_deref(),
            &container.labels,
        );
        // The RFC 5424 APP-NAME is truncated/sanitised at 48 chars, so
        // canonical identity rides in the metadata prefix instead. The
        // receiver strips it into `metadata_json.agent_docker`.
        let msg = format!("{AGENT_DOCKER_META_MARKER}{metadata}] {msg}");
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let line = format_rfc5424(
            pri,
            &ts,
            hostname,
            &container.app_name,
            &container.id[..12],
            &msg,
        );
        sender.try_send(line);
    }
    Ok(())
}

async fn list_containers(docker: &Docker) -> Result<Vec<ContainerInfo>> {
    let opts = ListContainersOptionsBuilder::default().all(false).build();
    let summaries = docker.list_containers(Some(opts)).await?;
    Ok(summaries
        .into_iter()
        .filter_map(|s| {
            let id = s.id?;
            let name = container_display_name(&id, s.names);
            let labels: HashMap<String, String> = s.labels.unwrap_or_default();
            if !should_forward_container_logs(&name, &labels) {
                return None;
            }
            let app_name = container_app_name(&name, &labels);
            Some(ContainerInfo {
                id,
                name,
                app_name,
                image: s.image,
                labels,
            })
        })
        .collect())
}

fn container_display_name(id: &str, names: Option<Vec<String>>) -> String {
    names
        .and_then(|ns| ns.into_iter().next())
        .map(|n| n.trim_start_matches('/').to_string())
        .unwrap_or_else(|| id.chars().take(12).collect())
}

/// Structured agent-attested Docker identity metadata for one log line.
/// This is the canonical resolver proof shape: `metadata_json.agent_docker`
/// with required host/container_id/container_name/stream and optional
/// compose_project/compose_service/image.
fn container_identity_metadata(
    host: &str,
    container_id: &str,
    container_name: &str,
    stream: &str,
    image: Option<&str>,
    labels: &HashMap<String, String>,
) -> serde_json::Value {
    serde_json::json!({
        "source_kind": "agent-docker",
        "agent_docker": {
            "host": host,
            "container_id": container_id,
            "container_name": container_name,
            "compose_project": labels.get("com.docker.compose.project"),
            "compose_service": labels.get("com.docker.compose.service"),
            "image": image,
            "stream": stream,
        }
    })
}

fn container_app_name(name: &str, labels: &HashMap<String, String>) -> String {
    match (
        labels.get("com.docker.compose.project"),
        labels.get("com.docker.compose.service"),
    ) {
        (Some(proj), Some(svc)) => format!("{proj}/{svc}/{name}"),
        (_, Some(svc)) => format!("{svc}/{name}"),
        _ => name.to_string(),
    }
}

fn should_forward_container_logs(name: &str, labels: &HashMap<String, String>) -> bool {
    if name == "cortex" {
        return false;
    }

    !matches!(
        (
            labels.get("com.docker.compose.project").map(String::as_str),
            labels.get("com.docker.compose.service").map(String::as_str),
        ),
        (Some("cortex"), Some("cortex"))
    )
}

fn connect(docker_url: &str) -> Result<Docker> {
    if docker_url.starts_with("unix://") || docker_url.starts_with("npipe://") {
        return Docker::connect_with_socket(docker_url, 120, bollard::API_DEFAULT_VERSION)
            .context("bollard socket connect");
    }

    use hyper_util::client::legacy::Client;
    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::rt::TokioExecutor;
    use std::sync::Arc as StdArc;

    let mut http = HttpConnector::new();
    http.set_keepalive(Some(Duration::from_secs(30)));
    http.set_keepalive_interval(Some(Duration::from_secs(30)));
    http.set_keepalive_retries(Some(3));

    let client = StdArc::new(Client::builder(TokioExecutor::new()).build(http));
    let url = docker_url.to_string();
    Docker::connect_with_custom_transport(
        move |req| {
            let client = StdArc::clone(&client);
            Box::pin(async move {
                client
                    .request(req)
                    .await
                    .map_err(bollard::errors::Error::from)
            })
        },
        Some(url),
        120,
        bollard::API_DEFAULT_VERSION,
    )
    .context("bollard http connect")
}

#[cfg(test)]
#[path = "docker_tests.rs"]
mod tests;
