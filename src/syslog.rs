use anyhow::Result;
use std::sync::{Arc, Mutex};
use tracing::{error, info};

use crate::config::{StorageConfig, SyslogConfig};
use crate::db::{self, DbPool};
use crate::ingest;
use crate::observability::RuntimeObservability;

pub(crate) mod enrichment;
mod listener;
mod parser;
pub(crate) mod writer;

pub async fn start_with_storage_state(
    config: SyslogConfig,
    storage: StorageConfig,
    pool: Arc<DbPool>,
    storage_state: Arc<Mutex<Option<db::StorageBudgetState>>>,
) -> Result<()> {
    // Default enrichment for the legacy convenience entry point: Authelia
    // and AdGuard reclassification are still applied (gating is "ungated"
    // when no source-IP prefix is configured), but secret scrubbing stays
    // on (default). Production runtime uses RuntimeCore which builds
    // enrichment from Config including operator overrides.
    let ingest_tx = ingest::start_writer_from_syslog_config(
        &config,
        storage,
        pool,
        storage_state,
        crate::syslog::enrichment::EnrichmentConfig::default(),
        Arc::new(RuntimeObservability::default()),
    );
    start_listeners(config, ingest_tx).await
}

pub(crate) async fn start_listeners(config: SyslogConfig, ingest: ingest::IngestTx) -> Result<()> {
    let bind_addr = config.bind_addr();

    let udp_ingest = ingest.clone();
    let udp_bind = bind_addr.clone();
    let max_size = config.max_message_size;
    tokio::spawn(async move {
        if let Err(e) = listener::udp_listener(&udp_bind, max_size, udp_ingest).await {
            error!(error = %e, "UDP syslog listener failed");
        }
    });

    let tcp_ingest = ingest.clone();
    let tcp_bind = bind_addr.clone();
    let max_tcp_connections = config.max_tcp_connections;
    let tcp_idle_timeout_secs = config.tcp_idle_timeout_secs;
    tokio::spawn(async move {
        if let Err(e) = listener::tcp_listener(
            &tcp_bind,
            tcp_ingest,
            max_size,
            max_tcp_connections,
            tcp_idle_timeout_secs,
        )
        .await
        {
            error!(error = %e, "TCP syslog listener failed");
        }
    });

    info!(
        bind = %bind_addr,
        max_message_size = config.max_message_size,
        max_tcp_connections = config.max_tcp_connections,
        tcp_idle_timeout_secs = config.tcp_idle_timeout_secs,
        write_channel_capacity = config.write_channel_capacity,
        "Syslog listeners started"
    );

    Ok(())
}
