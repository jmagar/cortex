use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bollard::query_parameters::{
    EventsOptions, EventsOptionsBuilder, ListContainersOptionsBuilder, LogsOptions,
    LogsOptionsBuilder,
};
use bollard::{BollardRequest, Docker};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

use super::models::ContainerMeta;

#[derive(Clone)]
pub(super) struct DockerHostClient {
    docker: Docker,
}

impl DockerHostClient {
    pub(super) fn connect(base_url: &str) -> Result<Self> {
        // Build an HttpConnector with TCP keepalive enabled. Bollard's stock
        // `connect_with_http` uses `HttpConnector::new()` which leaves SO_KEEPALIVE
        // off, so idle streaming connections (quiet container log streams, the
        // events stream when no events fire) get silently dropped by NATs,
        // conntrack, gateways, or peer-side idle timers, surfacing later as
        // "error reading a body from connection". Matches docker/cli PR #415.
        let mut http_connector = HttpConnector::new();
        http_connector.set_keepalive(Some(Duration::from_secs(30)));
        http_connector.set_keepalive_interval(Some(Duration::from_secs(30)));
        http_connector.set_keepalive_retries(Some(3));

        let mut client_builder = Client::builder(TokioExecutor::new());
        client_builder.pool_max_idle_per_host(0);
        let client = Arc::new(client_builder.build(http_connector));

        let docker = Docker::connect_with_custom_transport(
            move |req: BollardRequest| {
                let client = Arc::clone(&client);
                Box::pin(async move {
                    client
                        .request(req)
                        .await
                        .map_err(bollard::errors::Error::from)
                })
            },
            Some(base_url.to_string()),
            120,
            bollard::API_DEFAULT_VERSION,
        )?;

        Ok(Self { docker })
    }

    pub(super) async fn list_containers(&self) -> Result<Vec<ContainerMeta>> {
        let options = ListContainersOptionsBuilder::default().all(false).build();
        let containers = self.docker.list_containers(Some(options)).await?;
        Ok(containers
            .into_iter()
            .filter_map(ContainerMeta::from_summary)
            .collect())
    }

    pub(super) fn logs_options(since_unix: i64) -> LogsOptions {
        LogsOptionsBuilder::default()
            .stdout(true)
            .stderr(true)
            .timestamps(true)
            .follow(true)
            .since(since_unix.clamp(0, i32::MAX as i64) as i32)
            .tail("all")
            .build()
    }

    pub(super) fn container_events_options(since_unix: i64) -> EventsOptions {
        let mut filters: HashMap<String, Vec<String>> = HashMap::new();
        filters.insert("type".to_string(), vec!["container".to_string()]);
        EventsOptionsBuilder::default()
            .filters(&filters)
            .since(&since_unix.max(0).to_string())
            .build()
    }

    pub(super) fn docker(&self) -> Docker {
        self.docker.clone()
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
