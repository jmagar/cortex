mod correlate;
mod error;
mod models;
mod service;
mod time;

pub use correlate::severity_at_or_above;
pub use error::{ServiceError, ServiceResult};
pub use models::{
    AiSessionEntry, AnomaliesRequest, AnomaliesResponse, ClockSkewRequest, ClockSkewResponse,
    CompareRequest, CompareResponse, ContextRequest, ContextResponse, CorrelateEventsRequest,
    CorrelateEventsResponse, CorrelatedHost, DbStats, ErrorSummaryEntry, GetErrorsRequest,
    GetErrorsResponse, GetLogRequest, GetLogResponse, HostEntry, IngestRateRequest,
    IngestRateResponse, ListAppsRequest, ListAppsResponse, ListHostsResponse, ListSessionsRequest,
    ListSessionsResponse, ListSourceIpsResponse, LogEntry, PatternsRequest, PatternsResponse,
    SearchLogsRequest, SearchLogsResponse, SilentHostsRequest, SilentHostsResponse,
    TailLogsRequest, TimelineRequest, TimelineResponse,
};
pub use service::SyslogService;
pub use time::parse_optional_timestamp;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
