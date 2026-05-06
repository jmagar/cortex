use std::collections::HashMap;

use anyhow::Result;
use bollard::query_parameters::{
    EventsOptions, EventsOptionsBuilder, ListContainersOptionsBuilder, LogsOptions,
    LogsOptionsBuilder,
};
use bollard::Docker;

use super::models::ContainerMeta;

#[derive(Clone)]
pub(super) struct DockerHostClient {
    docker: Docker,
    // Separate client with no read timeout for follow=true log streams.
    // The api client (120s) would time out quiet containers after 2 minutes.
    streaming_docker: Docker,
}

impl DockerHostClient {
    pub(super) fn connect(base_url: &str) -> Result<Self> {
        let docker = Docker::connect_with_http(base_url, 120, bollard::API_DEFAULT_VERSION)?;
        let streaming_docker =
            Docker::connect_with_http(base_url, 0, bollard::API_DEFAULT_VERSION)?;
        Ok(Self {
            docker,
            streaming_docker,
        })
    }

    pub(super) async fn list_containers(&self) -> Result<Vec<ContainerMeta>> {
        let options = ListContainersOptionsBuilder::default().all(true).build();
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

    pub(super) fn streaming_docker(&self) -> Docker {
        self.streaming_docker.clone()
    }
}
