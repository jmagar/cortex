use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::config::{StorageConfig, SyslogConfig};
use crate::db::{self, DbPool};
use crate::observability::RuntimeObservability;
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
    observability: Arc<RuntimeObservability>,
}

impl IngestTx {
    pub(crate) async fn send(
        &self,
        entry: db::LogBatchEntry,
    ) -> Result<(), mpsc::error::SendError<db::LogBatchEntry>> {
        let result = self.tx.send(entry).await;
        let depth = self.queue_depth();
        match &result {
            Ok(()) => self.observability.record_enqueue_ok(depth),
            Err(_) => self.observability.record_enqueue_error(depth),
        }
        result
    }

    /// Non-blocking send. Returns `Err(TrySendErr::Full)` when the channel is
    /// at capacity so the OTLP HTTP handler can return 503 instead of awaiting
    /// and holding the connection open. The dropped entry is not returned —
    /// the caller's contract is "best effort, drop on backpressure."
    pub(crate) fn try_send(&self, entry: db::LogBatchEntry) -> Result<(), TrySendErr> {
        match self.tx.try_send(entry) {
            Ok(()) => {
                self.observability.record_enqueue_ok(self.queue_depth());
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.observability.record_enqueue_error(self.queue_depth());
                Err(TrySendErr::Full)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.observability.record_enqueue_error(self.queue_depth());
                Err(TrySendErr::Closed)
            }
        }
    }

    /// Best-effort current channel capacity (slots currently free). Used by
    /// the OTLP handler to pre-flight a multi-record batch and reject with
    /// 503 *before* any partial enqueue, avoiding duplicate-on-retry.
    pub(crate) fn capacity(&self) -> usize {
        self.tx.capacity()
    }

    pub(crate) fn queue_depth(&self) -> usize {
        WRITE_CHANNEL_CAPACITY.saturating_sub(self.tx.capacity())
    }

    pub(crate) fn observability(&self) -> Arc<RuntimeObservability> {
        Arc::clone(&self.observability)
    }

    /// Test-only constructor: builds an `IngestTx` from a raw sender so tests
    /// don't have to spawn a real batch writer.
    #[cfg(test)]
    pub(crate) fn from_sender_for_test(tx: mpsc::Sender<db::LogBatchEntry>) -> Self {
        let observability = Arc::new(RuntimeObservability::default());
        observability.set_queue_capacity(WRITE_CHANNEL_CAPACITY);
        Self { tx, observability }
    }
}

pub(crate) fn start_writer(
    storage: StorageConfig,
    pool: Arc<DbPool>,
    storage_state: Arc<Mutex<Option<db::StorageBudgetState>>>,
    batch_size: usize,
    flush_interval_ms: u64,
    enrichment: EnrichmentConfig,
    observability: Arc<RuntimeObservability>,
) -> IngestTx {
    let (tx, rx) = mpsc::channel::<db::LogBatchEntry>(WRITE_CHANNEL_CAPACITY);
    observability.set_queue_capacity(WRITE_CHANNEL_CAPACITY);
    let writer_observability = Arc::clone(&observability);
    tokio::spawn(async move {
        let context = syslog::writer::WriterContext::new(
            pool,
            storage,
            storage_state,
            enrichment,
            writer_observability,
        );
        syslog::writer::batch_writer(
            rx,
            context,
            batch_size,
            tokio::time::Duration::from_millis(flush_interval_ms),
        )
        .await;
    });
    IngestTx { tx, observability }
}

pub(crate) fn start_writer_from_syslog_config(
    syslog: &SyslogConfig,
    storage: StorageConfig,
    pool: Arc<DbPool>,
    storage_state: Arc<Mutex<Option<db::StorageBudgetState>>>,
    enrichment: EnrichmentConfig,
    observability: Arc<RuntimeObservability>,
) -> IngestTx {
    start_writer(
        storage,
        pool,
        storage_state,
        syslog.batch_size,
        syslog.flush_interval,
        enrichment,
        observability,
    )
}
