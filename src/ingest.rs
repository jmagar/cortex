use std::sync::Arc;

use parking_lot::Mutex;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::config::{ReceiverConfig, StorageConfig};
use crate::db::{self, DbPool};
use crate::enrich::EnrichmentPipeline;
use crate::observability::RuntimeObservability;
use crate::receiver;
use crate::receiver::enrichment::EnrichmentConfig;

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
    channel_capacity: usize,
    /// Shutdown signal. Sending `true` tells the batch writer to drain and
    /// exit. `watch` is used so the signal can be sent once and received by
    /// the writer regardless of clone count. `Arc` is needed because
    /// `watch::Sender` is not `Clone`.
    shutdown_tx: Arc<watch::Sender<bool>>,
    /// Handle for the batch writer task. Stored so `shutdown` can await the
    /// writer's actual completion rather than sleeping a fixed duration.
    writer_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

struct WriterTuning {
    batch_size: usize,
    flush_interval_ms: u64,
    channel_capacity: usize,
}

impl WriterTuning {
    fn from_receiver_config(config: &ReceiverConfig) -> Self {
        Self {
            batch_size: config.batch_size,
            flush_interval_ms: config.flush_interval,
            channel_capacity: config.write_channel_capacity,
        }
    }
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
        self.channel_capacity.saturating_sub(self.tx.capacity())
    }

    pub(crate) fn queue_capacity(&self) -> usize {
        self.channel_capacity
    }

    pub(crate) fn observability(&self) -> Arc<RuntimeObservability> {
        Arc::clone(&self.observability)
    }

    /// Signal the batch writer to drain and exit, then await its completion
    /// up to `timeout`. Listener tasks hold their own `IngestTx` clones; this
    /// drops only the one sender held by `RuntimeCore`. The mpsc channel remains
    /// open until all listener clones are also dropped (by their task futures
    /// completing), but the `shutdown_tx` signal causes the writer to drain
    /// and return before that happens.
    pub(crate) async fn shutdown(self, timeout: std::time::Duration) {
        let _ = self.shutdown_tx.send(true);
        // Drop our tx clone — not strictly required since the shutdown arm in
        // the writer doesn't wait for EOF, but good hygiene.
        let handle = self.writer_handle.lock().take();
        drop(self.tx);
        if let Some(handle) = handle {
            let _ = tokio::time::timeout(timeout, handle).await;
        }
    }

    /// Test-only constructor: builds an `IngestTx` from a raw sender so tests
    /// don't have to spawn a real batch writer.
    #[cfg(test)]
    pub(crate) fn from_sender_for_test(tx: mpsc::Sender<db::LogBatchEntry>) -> Self {
        let observability = Arc::new(RuntimeObservability::default());
        let channel_capacity = tx.max_capacity();
        observability.set_queue_capacity(channel_capacity);
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            tx,
            observability,
            channel_capacity,
            shutdown_tx: Arc::new(shutdown_tx),
            writer_handle: Arc::new(Mutex::new(None)),
        }
    }
}

fn start_writer(
    storage: StorageConfig,
    pool: Arc<DbPool>,
    storage_state: Arc<Mutex<Option<db::StorageBudgetState>>>,
    tuning: WriterTuning,
    enrichment: EnrichmentConfig,
    observability: Arc<RuntimeObservability>,
) -> IngestTx {
    let WriterTuning {
        batch_size,
        flush_interval_ms,
        channel_capacity,
    } = tuning;
    let (tx, rx) = mpsc::channel::<db::LogBatchEntry>(channel_capacity);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    observability.set_queue_capacity(channel_capacity);
    let writer_observability = Arc::clone(&observability);
    let handle = tokio::spawn(async move {
        let context = receiver::writer::WriterContext::new(
            pool,
            storage,
            storage_state,
            enrichment,
            Arc::new(EnrichmentPipeline::new()),
            writer_observability,
        );
        receiver::writer::batch_writer(
            rx,
            context,
            batch_size,
            tokio::time::Duration::from_millis(flush_interval_ms),
            shutdown_rx,
        )
        .await;
    });
    IngestTx {
        tx,
        observability,
        channel_capacity,
        shutdown_tx: Arc::new(shutdown_tx),
        writer_handle: Arc::new(Mutex::new(Some(handle))),
    }
}

pub(crate) fn start_writer_from_receiver_config(
    syslog: &ReceiverConfig,
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
        WriterTuning::from_receiver_config(syslog),
        enrichment,
        observability,
    )
}
