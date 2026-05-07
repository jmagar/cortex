use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::config::{StorageConfig, SyslogConfig};
use crate::db::{self, DbPool};
use crate::syslog;
use crate::syslog::enrichment::EnrichmentConfig;

pub const WRITE_CHANNEL_CAPACITY: usize = 10_000;

/// Lightweight error type for [`IngestTx::try_send`] — the entry is dropped on
/// backpressure rather than returned to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrySendErr {
    Full,
    Closed,
}

#[derive(Clone)]
pub(crate) struct IngestTx {
    tx: mpsc::Sender<db::LogBatchEntry>,
}

impl IngestTx {
    pub(crate) async fn send(
        &self,
        entry: db::LogBatchEntry,
    ) -> Result<(), mpsc::error::SendError<db::LogBatchEntry>> {
        self.tx.send(entry).await
    }

    /// Non-blocking send. Returns `Err(TrySendErr::Full)` when the channel is
    /// at capacity so the OTLP HTTP handler can return 503 instead of awaiting
    /// and holding the connection open. The dropped entry is not returned —
    /// the caller's contract is "best effort, drop on backpressure."
    pub(crate) fn try_send(&self, entry: db::LogBatchEntry) -> Result<(), TrySendErr> {
        match self.tx.try_send(entry) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(TrySendErr::Full),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(TrySendErr::Closed),
        }
    }

    pub(crate) fn sender(&self) -> mpsc::Sender<db::LogBatchEntry> {
        self.tx.clone()
    }

    /// Test-only constructor: builds an `IngestTx` from a raw sender so tests
    /// don't have to spawn a real batch writer.
    #[cfg(test)]
    pub(crate) fn from_sender_for_test(tx: mpsc::Sender<db::LogBatchEntry>) -> Self {
        Self { tx }
    }
}

pub(crate) fn start_writer(
    storage: StorageConfig,
    pool: Arc<DbPool>,
    storage_state: Arc<Mutex<Option<db::StorageBudgetState>>>,
    batch_size: usize,
    flush_interval_ms: u64,
    enrichment: EnrichmentConfig,
) -> IngestTx {
    let (tx, rx) = mpsc::channel::<db::LogBatchEntry>(WRITE_CHANNEL_CAPACITY);
    tokio::spawn(async move {
        syslog::writer::batch_writer(
            rx,
            pool,
            storage,
            storage_state,
            batch_size,
            tokio::time::Duration::from_millis(flush_interval_ms),
            enrichment,
        )
        .await;
    });
    IngestTx { tx }
}

pub(crate) fn start_writer_from_syslog_config(
    syslog: &SyslogConfig,
    storage: StorageConfig,
    pool: Arc<DbPool>,
    storage_state: Arc<Mutex<Option<db::StorageBudgetState>>>,
    enrichment: EnrichmentConfig,
) -> IngestTx {
    start_writer(
        storage,
        pool,
        storage_state,
        syslog.batch_size,
        syslog.flush_interval,
        enrichment,
    )
}
