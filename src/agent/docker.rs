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

struct ContainerInfo {
    id: String,
    name: String,
    app_name: String,
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
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let line = format_rfc5424(
            pri,
            &ts,
            hostname,
            &container.app_name,
            &container.id[..12],
            msg,
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
            let app_name = container_app_name(&name, &labels);
            Some(ContainerInfo { id, name, app_name })
        })
        .collect())
}

fn container_display_name(id: &str, names: Option<Vec<String>>) -> String {
    names
        .and_then(|ns| ns.into_iter().next())
        .map(|n| n.trim_start_matches('/').to_string())
        .unwrap_or_else(|| id.chars().take(12).collect())
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
mod tests {
    use super::*;

    #[test]
    fn container_display_name_prefers_first_docker_name_without_leading_slash() {
        assert_eq!(
            container_display_name(
                "abcdef1234567890",
                Some(vec!["/cortex".to_string(), "/alias".to_string()])
            ),
            "cortex"
        );
    }

    #[test]
    fn container_display_name_falls_back_to_short_id_when_names_missing() {
        assert_eq!(
            container_display_name("abcdef1234567890", Some(Vec::new())),
            "abcdef123456"
        );
        assert_eq!(container_display_name("short", None), "short");
    }

    #[test]
    fn container_app_name_includes_compose_project_service_and_container_name() {
        let labels = HashMap::from([
            (
                "com.docker.compose.project".to_string(),
                "cortex".to_string(),
            ),
            (
                "com.docker.compose.service".to_string(),
                "server".to_string(),
            ),
        ]);

        assert_eq!(
            container_app_name("cortex-1", &labels),
            "cortex/server/cortex-1"
        );
    }

    #[test]
    fn container_app_name_falls_back_to_service_or_container_name() {
        let service_only = HashMap::from([(
            "com.docker.compose.service".to_string(),
            "server".to_string(),
        )]);

        assert_eq!(
            container_app_name("cortex-1", &service_only),
            "server/cortex-1"
        );
        assert_eq!(container_app_name("cortex-1", &HashMap::new()), "cortex-1");
    }
}
