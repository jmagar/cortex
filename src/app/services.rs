use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use chrono::{TimeDelta, Utc};
use tokio::sync::Semaphore;

const DB_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
const SLOW_DB_MS: u128 = 500;

use super::correlate::{group_by_host, severity_at_or_above};
use super::models::{
    AbuseSearchRequest, AbuseSearchResponse, AiAssessEvidenceSummary, AiAssessRequest,
    AiAssessResponse, AiCorrelateLimitPolicy, AiCorrelateRequest, AiCorrelateResponse,
    AiCorrelationAnchor, AiIncidentRequest, AiIncidentResponse, AiInvestigateRequest,
    AiInvestigateResponse, AiLimitPolicy, AiSessionEntry, AnomaliesRequest, AnomaliesResponse,
    AskHistoryRequest, AskHistoryResponse, ClockSkewRequest, ClockSkewResponse, CompareRequest,
    CompareResponse, ContextRequest, ContextResponse, CorrelateEventsRequest,
    CorrelateEventsResponse, CorrelateStateHostEntry, CorrelateStateRequest,
    CorrelateStateResponse, CorrelateStateWindow, CortexOverlaySummary, DbBackupResult,
    DbCheckpointRequest, DbCheckpointResult, DbIntegrityJobStarted, DbIntegrityResult,
    DbMaintenanceStatus, DbStats, DbVacuumRequest, DbVacuumResult, FilterLogsRequest,
    FleetStateHostRow, FleetStateRequest, FleetStateResponse, FleetStateSummary, GetErrorsRequest,
    GetErrorsResponse, GetLogRequest, GetLogResponse, GraphAroundRequest, GraphAroundResponse,
    GraphEntity, GraphEntityCandidate, GraphEntityLookupRequest, GraphEntityLookupResponse,
    GraphEntitySummary, GraphEvidence, GraphEvidenceLookupRequest, GraphEvidenceLookupResponse,
    GraphExplainRequest, GraphExplainResponse, GraphIncidentNarrative, GraphNarrativeChain,
    GraphNextQuery, GraphProjectionStatusResponse, GraphRebuildResponse, GraphRebuildStatsResponse,
    GraphRelationship, GraphResponseMetadata, GraphSourceLogSummary, HomelabMapAnswerRow,
    HomelabMapAnswerTruncation, HomelabMapGraphAnswer, HomelabMapGraphTarget, HomelabMapNextQuery,
    HomelabMapNode, HomelabMapProofQuery, HomelabMapRequest, HomelabMapResponse, HomelabMapSummary,
    IncidentContextRequest, IncidentContextResponse, IncidentEvent, IncidentRequest,
    IncidentResponse, IngestRateRequest, IngestRateResponse, ListAiProjectsRequest,
    ListAiProjectsResponse, ListAiToolsRequest, ListAiToolsResponse, ListAppsRequest,
    ListAppsResponse, ListHostsResponse, ListSessionsRequest, ListSessionsResponse,
    ListSourceIpsRequest, ListSourceIpsResponse, LogEntry, MaintenanceJobStatus,
    NotificationsRecentRequest, PatternsRequest, PatternsResponse, ProjectContextRequest,
    ProjectContextResponse, RequestActor, SearchLogsRequest, SearchLogsResponse,
    SearchSessionsRequest, SearchSessionsResponse, ServiceJournalEntry, ServiceLogsRequest,
    ServiceLogsResponse, SilentHostsRequest, SilentHostsResponse, SimilarIncidentsRequest,
    SimilarIncidentsResponse, TailLogsRequest, TimelineRequest, TimelineResponse, TopologyFinding,
    TopologyFindingEntity, TopologyFindingEvidence, UsageBlocksRequest, UsageBlocksResponse,
};
use super::os_adapter::{OsAdapter, SystemOsAdapter};
use super::time::{parse_optional_timestamp, parse_required_timestamp, rfc3339_z};
use super::{ServiceError, ServiceResult};
use crate::app::{correlate, heartbeat_flags, models, os_adapter, time};
use crate::assessment::{GeminiAssessConfig, build_assessment_prompt, run_gemini_assessment};
use crate::command_log::{self, CommandLogImportResult};
use crate::config::StorageConfig;
use crate::db::{self, Bucket, ContextRef, DbPool, SearchParams, TimelineGroupBy};
use crate::scanner;

mod ai;
mod ai_indexing;
mod analytics;
mod assessment;
mod compose;
mod error_detection;
mod filters;
mod graph;
mod graph_limits;
mod graph_safety;
mod graph_support;
mod imports;
mod incidents;
mod journal;
mod logs;
mod maintenance;
mod map;
mod map_answers;
mod map_findings;
mod rag;

pub use compose::run_compose_status;
pub use journal::run_service_logs;
#[cfg(test)]
use journal::{normalize_syslog_owned_service, parse_journal_json_lines};

/// Service-layer entry point bridging request structs to SQLite.
///
/// `Clone` is cheap because every field is either `Arc`-wrapped or a small
/// scalar. Public methods live in focused `services/*` modules; this file owns
/// construction and DB execution coordination.
#[derive(Clone)]
pub struct CortexService {
    pool: Arc<DbPool>,
    pub(super) storage: StorageConfig,
    db_permits: Arc<Semaphore>,
    acquire_timeout: Duration,
    /// OS-level adapter for journalctl / systemd shell-outs.
    pub(super) os: Arc<dyn OsAdapter + Send + Sync>,
}

/// Number of read permits issued for a given r2d2 pool size.
///
/// One connection is RESERVED for writers: the syslog batch writer (and other
/// ingest-side writers) call `pool.get()` directly without holding a service
/// permit, so issuing `pool_size` read permits let concurrent slow MCP reads
/// hold every connection — the writer then blocked up to the pool timeout per
/// flush, the ingest channel filled, and packets dropped (full-review PH3).
/// `pool_size - 1` guarantees the writer can always reach a connection within
/// its retry budget. Floor of 1 keeps single-connection test pools usable.
fn read_permits_for_pool(pool_size: u32) -> usize {
    (pool_size.saturating_sub(1)).max(1) as usize
}

impl CortexService {
    pub(crate) fn new(pool: Arc<DbPool>, storage: StorageConfig) -> Self {
        let permits = read_permits_for_pool(storage.pool_size);
        Self {
            pool,
            storage,
            db_permits: Arc::new(Semaphore::new(permits)),
            acquire_timeout: DB_ACQUIRE_TIMEOUT,
            os: Arc::new(SystemOsAdapter),
        }
    }

    /// Test constructor that injects a custom `OsAdapter`.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn with_os_adapter(
        pool: Arc<DbPool>,
        storage: StorageConfig,
        os: Arc<dyn OsAdapter + Send + Sync>,
    ) -> Self {
        let permits = read_permits_for_pool(storage.pool_size);
        Self {
            pool,
            storage,
            db_permits: Arc::new(Semaphore::new(permits)),
            acquire_timeout: DB_ACQUIRE_TIMEOUT,
            os,
        }
    }

    /// One-shot SQLite schema-version probe. Sync because callers run during
    /// startup construction (e.g. `ApiState::new` caches it for /api/version)
    /// before the runtime serves requests. Exists so transport layers never
    /// reach into `db::` directly (full-review AL1).
    pub fn schema_version(&self) -> anyhow::Result<i64> {
        Ok(crate::db::read_schema_version_info(&self.pool)?.version)
    }

    async fn run_db<F, T>(&self, op: &'static str, f: F) -> ServiceResult<T>
    where
        F: FnOnce(&DbPool) -> anyhow::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let wait_start = Instant::now();
        let permit_result = tokio::time::timeout(
            self.acquire_timeout,
            Arc::clone(&self.db_permits).acquire_owned(),
        )
        .await;
        let permit_ms = wait_start.elapsed().as_millis();

        let permit = match permit_result {
            Err(_) => {
                tracing::warn!(op, permit_ms, "db acquire timeout");
                return Err(ServiceError::Busy("database worker limit reached".into()));
            }
            Ok(Err(_)) => {
                tracing::warn!(op, permit_ms, "db semaphore closed");
                return Err(ServiceError::Busy("database worker limit closed".into()));
            }
            Ok(Ok(p)) => p,
        };

        let exec_start = Instant::now();
        let pool = Arc::clone(&self.pool);
        let join_result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f(&pool)
        })
        .await;
        let exec_ms = exec_start.elapsed().as_millis();

        let result = match join_result {
            Err(e) => {
                if e.is_cancelled() {
                    tracing::warn!(op, permit_ms, exec_ms, "db task cancelled");
                } else {
                    tracing::warn!(op, permit_ms, exec_ms, error = %e, "db task panic");
                }
                return Err(ServiceError::Internal(anyhow::anyhow!(
                    "Task join error: {e}"
                )));
            }
            // Preserve typed ServiceErrors raised inside the closure (e.g.
            // InvalidInput/NotFound from query helpers) instead of flattening
            // everything to Internal — the MCP surface maps each variant to a
            // distinct client-visible error class (full-review AH1).
            Ok(r) => r.map_err(|e| match e.downcast::<ServiceError>() {
                Ok(svc) => svc,
                Err(e) => {
                    tracing::debug!(error = %e, "anyhow error not a ServiceError, classifying as Internal");
                    ServiceError::Internal(e)
                }
            }),
        };

        if exec_ms > SLOW_DB_MS {
            match &result {
                Ok(_) => tracing::warn!(op, permit_ms, exec_ms, "db op ok"),
                Err(e) => tracing::warn!(op, permit_ms, exec_ms, error = %e, "db op err"),
            }
        } else {
            match &result {
                Ok(_) => tracing::debug!(op, permit_ms, exec_ms, "db op ok"),
                Err(e) => tracing::debug!(op, permit_ms, exec_ms, error = %e, "db op err"),
            }
        }
        result
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
