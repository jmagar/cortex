mod correlate;
mod error;
mod models;
mod service;
mod time;

pub use correlate::severity_at_or_above;
pub use error::{ServiceError, ServiceResult};
pub use models::{
    AiProjectEntry, AiSessionEntry, AiToolEntry, AnomaliesRequest, AnomaliesResponse,
    ClockSkewRequest, ClockSkewResponse, CompareRequest, CompareResponse, ContextRequest,
    ContextResponse, CorrelateEventsRequest, CorrelateEventsResponse, CorrelatedHost, DbStats,
    ErrorSummaryEntry, GetErrorsRequest, GetErrorsResponse, GetLogRequest, GetLogResponse,
    HostEntry, IngestRateRequest, IngestRateResponse, ListAiProjectsRequest,
    ListAiProjectsResponse, ListAiToolsRequest, ListAiToolsResponse, ListAppsRequest,
    ListAppsResponse, ListHostsResponse, ListSessionsRequest, ListSessionsResponse,
    ListSourceIpsResponse, LogEntry, PatternsRequest, PatternsResponse, ProjectContextRequest,
    ProjectContextResponse, SearchLogsRequest, SearchLogsResponse, SearchSessionsRequest,
    SearchSessionsResponse, SearchedSessionEntry, SilentHostsRequest, SilentHostsResponse,
    TailLogsRequest, TimelineRequest, TimelineResponse, UsageBlock, UsageBlocksRequest,
    UsageBlocksResponse,
};
pub use service::SyslogService;
pub use time::parse_optional_timestamp;

#[cfg(test)]
#[path = "app/mod_tests.rs"]
mod tests;
