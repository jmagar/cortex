mod correlate;
pub(crate) mod error_detection;
mod error;
mod models;
mod service;
mod time;

pub use correlate::severity_at_or_above;
pub use error::{ServiceError, ServiceResult};
pub use models::{
    AbuseMatch, AbuseSearchRequest, AbuseSearchResponse, AiCorrelateRequest, AiCorrelateResponse,
    AiCorrelationAnchor, AiProjectEntry, AiSessionEntry, AiToolEntry, AnomaliesRequest,
    AnomaliesResponse, ClockSkewRequest, ClockSkewResponse, CompareRequest, CompareResponse,
    ContextRequest, ContextResponse, CorrelateEventsRequest, CorrelateEventsResponse,
    CorrelatedHost, DbBackupResult, DbCheckpointResult, DbIntegrityResult, DbMaintenanceStatus,
    DbStats, DbVacuumResult, ErrorSummaryEntry, GetErrorsRequest, GetErrorsResponse, GetLogRequest,
    GetLogResponse, HostEntry, IngestRateRequest, IngestRateResponse, ListAiProjectsRequest,
    ListAiProjectsResponse, ListAiToolsRequest, ListAiToolsResponse, ListAppsRequest,
    ListAppsResponse, ListHostsResponse, ListSessionsRequest, ListSessionsResponse,
    ListSourceIpsResponse, LogEntry, PatternsRequest, PatternsResponse, ProjectContextRequest,
    ProjectContextResponse, SearchLogsRequest, SearchLogsResponse, SearchSessionsRequest,
    SearchSessionsResponse, SearchedSessionEntry, SilentHostsRequest, SilentHostsResponse,
    TailLogsRequest, TimelineRequest, TimelineResponse, UsageBlock, UsageBlocksRequest,
    UsageBlocksResponse,
    // Error detection
    AckErrorRequest, AckErrorResponse, ErrorSignatureEntry, UnackErrorRequest, UnackErrorResponse,
    UnaddressedErrorsRequest, UnaddressedErrorsResponse,
};
pub use service::SyslogService;
pub use time::parse_optional_timestamp;

#[cfg(test)]
#[path = "app/mod_tests.rs"]
mod tests;
