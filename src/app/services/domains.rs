use super::*;
use crate::app::{
    AckErrorRequest, AckErrorResponse, AiCorrelateLimitPolicy, AiCorrelateRequest,
    AiCorrelateResponse, AnomaliesRequest, AnomaliesResponse, ClockSkewRequest, ClockSkewResponse,
    CompareRequest, CompareResponse, CorrelateEventsRequest, CorrelateEventsResponse,
    CorrelateStateRequest, CorrelateStateResponse, DbStats, FileTailRequest, FileTailResponse,
    FleetStateRequest, FleetStateResponse, GetErrorsRequest, GetErrorsResponse, HostStateRequest,
    HostStateResponse, IncidentContextRequest, IncidentContextResponse, IncidentRequest,
    IncidentResponse, IngestRateRequest, IngestRateResponse, ListHostsResponse,
    ListSourceIpsRequest, ListSourceIpsResponse, NotificationsRecentRequest, PatternsRequest,
    PatternsResponse, RequestActor, ServiceLogsRequest, ServiceLogsResponse, SilentHostsRequest,
    SilentHostsResponse, SimilarIncidentsRequest, SimilarIncidentsResponse, TimelineRequest,
    TimelineResponse, TopicCorrelateRequest, TopicCorrelateResponse, UnackErrorRequest,
    UnackErrorResponse, UnaddressedErrorsRequest, UnaddressedErrorsResponse,
};

pub struct HostsDomain<'a> {
    service: &'a CortexService,
}

pub struct AnalysisDomain<'a> {
    service: &'a CortexService,
}

pub struct CorrelateDomain<'a> {
    service: &'a CortexService,
}

pub struct StateDomain<'a> {
    service: &'a CortexService,
}

pub struct StatsDomain<'a> {
    service: &'a CortexService,
}

pub struct IngestDomain<'a> {
    service: &'a CortexService,
}

pub struct AlertsDomain<'a> {
    service: &'a CortexService,
}

pub struct ComposeDomain<'a> {
    service: &'a CortexService,
}

impl CortexService {
    pub fn hosts(&self) -> HostsDomain<'_> {
        HostsDomain { service: self }
    }

    pub fn analysis(&self) -> AnalysisDomain<'_> {
        AnalysisDomain { service: self }
    }

    pub fn correlate(&self) -> CorrelateDomain<'_> {
        CorrelateDomain { service: self }
    }

    pub fn state(&self) -> StateDomain<'_> {
        StateDomain { service: self }
    }

    pub fn stats(&self) -> StatsDomain<'_> {
        StatsDomain { service: self }
    }

    pub fn ingest(&self) -> IngestDomain<'_> {
        IngestDomain { service: self }
    }

    pub fn alerts(&self) -> AlertsDomain<'_> {
        AlertsDomain { service: self }
    }

    pub fn compose(&self) -> ComposeDomain<'_> {
        ComposeDomain { service: self }
    }
}

impl HostsDomain<'_> {
    pub async fn list(&self) -> ServiceResult<ListHostsResponse> {
        self.service.list_hosts().await
    }

    pub async fn source_ips(
        &self,
        req: ListSourceIpsRequest,
    ) -> ServiceResult<ListSourceIpsResponse> {
        self.service.list_source_ips(req).await
    }

    pub async fn silent(&self, req: SilentHostsRequest) -> ServiceResult<SilentHostsResponse> {
        self.service.silent_hosts(req).await
    }
}

impl AnalysisDomain<'_> {
    pub async fn errors(&self, req: GetErrorsRequest) -> ServiceResult<GetErrorsResponse> {
        self.service.get_errors(req).await
    }

    pub async fn patterns(&self, req: PatternsRequest) -> ServiceResult<PatternsResponse> {
        self.service.patterns(req).await
    }

    pub async fn anomalies(&self, req: AnomaliesRequest) -> ServiceResult<AnomaliesResponse> {
        self.service.anomalies(req).await
    }

    pub async fn compare(&self, req: CompareRequest) -> ServiceResult<CompareResponse> {
        self.service.compare(req).await
    }

    pub async fn incident(&self, req: IncidentRequest) -> ServiceResult<IncidentResponse> {
        self.service.incident(req).await
    }

    pub async fn similar_incidents(
        &self,
        req: SimilarIncidentsRequest,
    ) -> ServiceResult<SimilarIncidentsResponse> {
        self.service.similar_incidents(req).await
    }

    pub async fn incident_context(
        &self,
        req: IncidentContextRequest,
    ) -> ServiceResult<IncidentContextResponse> {
        self.service.incident_context(req).await
    }
}

impl CorrelateDomain<'_> {
    pub async fn events(
        &self,
        req: CorrelateEventsRequest,
    ) -> ServiceResult<CorrelateEventsResponse> {
        self.service.correlate_events(req).await
    }

    pub async fn state(&self, req: CorrelateStateRequest) -> ServiceResult<CorrelateStateResponse> {
        self.service.correlate_state(req).await
    }

    pub async fn ai(&self, req: AiCorrelateRequest) -> ServiceResult<AiCorrelateResponse> {
        self.service.correlate_ai_logs(req).await
    }

    pub async fn ai_with_limit_policy(
        &self,
        req: AiCorrelateRequest,
        policy: AiCorrelateLimitPolicy,
    ) -> ServiceResult<AiCorrelateResponse> {
        self.service
            .correlate_ai_logs_with_limit_policy(req, policy)
            .await
    }

    pub async fn topic(&self, req: TopicCorrelateRequest) -> ServiceResult<TopicCorrelateResponse> {
        self.service.topic_correlate(req).await
    }
}

impl StateDomain<'_> {
    pub async fn host(&self, req: HostStateRequest) -> ServiceResult<HostStateResponse> {
        self.service.host_state(req).await
    }

    pub async fn fleet(&self, req: FleetStateRequest) -> ServiceResult<FleetStateResponse> {
        self.service.fleet_state(req).await
    }

    pub async fn clock_skew(&self, req: ClockSkewRequest) -> ServiceResult<ClockSkewResponse> {
        self.service.clock_skew(req).await
    }
}

impl StatsDomain<'_> {
    pub async fn summary(&self) -> ServiceResult<DbStats> {
        self.service.get_stats().await
    }

    pub async fn ingest_rate(&self, req: IngestRateRequest) -> ServiceResult<IngestRateResponse> {
        self.service.ingest_rate(req).await
    }

    pub async fn timeline(&self, req: TimelineRequest) -> ServiceResult<TimelineResponse> {
        self.service.timeline(req).await
    }
}

impl IngestDomain<'_> {
    pub async fn file_tails(&self, req: FileTailRequest) -> ServiceResult<FileTailResponse> {
        self.service.file_tails(req).await
    }

    pub async fn shell_history(
        &self,
        path: PathBuf,
        shell: String,
    ) -> ServiceResult<CommandLogImportResult> {
        self.service.import_shell_history(path, shell).await
    }

    pub async fn atuin_history(&self, path: PathBuf) -> ServiceResult<CommandLogImportResult> {
        self.service.import_atuin_history(path).await
    }

    pub async fn agent_command_spool(
        &self,
        path: PathBuf,
    ) -> ServiceResult<CommandLogImportResult> {
        self.service.import_agent_command_spool(path).await
    }
}

impl AlertsDomain<'_> {
    pub async fn signatures(
        &self,
        req: UnaddressedErrorsRequest,
    ) -> ServiceResult<UnaddressedErrorsResponse> {
        self.service.unaddressed_errors(req).await
    }

    pub async fn ack_signature(
        &self,
        req: AckErrorRequest,
        actor: impl Into<RequestActor>,
    ) -> ServiceResult<AckErrorResponse> {
        self.service.ack_error(req, actor).await
    }

    pub async fn unack_signature(
        &self,
        req: UnackErrorRequest,
        actor: impl Into<RequestActor>,
    ) -> ServiceResult<UnackErrorResponse> {
        self.service.unack_error(req, actor).await
    }

    pub async fn notifications(
        &self,
        req: NotificationsRecentRequest,
    ) -> ServiceResult<Vec<crate::db::notifications::FiringRow>> {
        self.service.notifications_recent_checked(req).await
    }

    pub async fn test_notification(
        &self,
        body: String,
        actor: impl Into<RequestActor>,
        config: &crate::config::NotificationsConfig,
    ) -> ServiceResult<String> {
        self.service
            .notifications_test_checked(body, actor, config)
            .await
    }
}

impl ComposeDomain<'_> {
    pub async fn status(&self) -> ServiceResult<crate::compose::ComposeStatus> {
        run_compose_status().await
    }

    pub async fn service_logs(
        &self,
        req: ServiceLogsRequest,
    ) -> ServiceResult<ServiceLogsResponse> {
        self.service.service_logs(req).await
    }
}
