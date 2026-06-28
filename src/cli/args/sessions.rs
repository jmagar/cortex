#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionsCommand {
    List(super::SessionsArgs),
    Search(SessionsSearchArgs),
    Abuse(SessionsAbuseArgs),
    Correlate(SessionsCorrelateArgs),
    Blocks(SessionsBlocksArgs),
    Context(SessionsContextArgs),
    Tools(SessionsListArgs),
    Projects(SessionsListArgs),
    Index(SessionsIndexArgs),
    Add(SessionsAddArgs),
    Watch(SessionsWatchArgs),
    Checkpoints(SessionsCheckpointsArgs),
    Errors(SessionsErrorsArgs),
    PruneCheckpoints(SessionsPruneCheckpointsArgs),
    Doctor(SessionsDoctorArgs),
    WatchStatus(super::OutputArgs),
    SmokeWatch(super::OutputArgs),
    SimilarIncidents(SessionsSimilarArgs),
    AskHistory(SessionsAskHistoryArgs),
    IncidentContext(SessionsIncidentContextArgs),
    Incidents(SessionsIncidentsArgs),
    Investigate(SessionsInvestigateArgs),
    Assess(SessionsAssessArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsDoctorArgs {
    pub json: bool,
    pub strict_permissions: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsSearchArgs {
    pub query: String,
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsAbuseArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
    pub before: Option<u32>,
    pub after: Option<u32>,
    pub terms: Vec<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsCorrelateArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub session_id: Option<String>,
    pub ai_query: Option<String>,
    pub log_query: Option<String>,
    pub host: Option<String>,
    pub source: Option<String>,
    pub app: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub window_minutes: Option<u32>,
    pub severity_min: Option<String>,
    pub limit: Option<u32>,
    pub events_per_anchor: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsBlocksArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<usize>,
    pub detail: SessionsOutputDetail,
    pub json: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SessionsOutputDetail {
    #[default]
    Compact,
    Full,
}

impl SessionsOutputDetail {
    pub(crate) fn parse(value: &str, flag: &str) -> anyhow::Result<Self> {
        match value {
            "compact" => Ok(Self::Compact),
            "full" => Ok(Self::Full),
            _ => anyhow::bail!("{flag} must be compact or full"),
        }
    }

    pub(crate) fn is_compact(self) -> bool {
        matches!(self, Self::Compact)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsContextArgs {
    pub project: String,
    pub tool: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsListArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsIndexArgs {
    pub path: Option<String>,
    pub force: bool,
    pub since: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsAddArgs {
    pub file: String,
    pub force: bool,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionsWatchArgs {
    pub path: Option<String>,
    pub debounce_ms: u64,
    pub settle_ms: u64,
    pub max_retries: u8,
    pub no_initial_scan: bool,
    pub json: bool,
}

impl Default for SessionsWatchArgs {
    fn default() -> Self {
        Self {
            path: None,
            debounce_ms: 750,
            settle_ms: 500,
            max_retries: 5,
            no_initial_scan: false,
            json: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsCheckpointsArgs {
    pub errors_only: bool,
    pub missing_only: bool,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsErrorsArgs {
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsPruneCheckpointsArgs {
    pub missing_only: bool,
    pub dry_run: bool,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsSimilarArgs {
    pub query: String,
    pub host: Option<String>,
    pub app: Option<String>,
    pub severity_min: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub window_minutes: Option<u32>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsAskHistoryArgs {
    pub query: String,
    pub host: Option<String>,
    pub app: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsIncidentContextArgs {
    pub since: String,
    pub until: String,
    pub host: Option<String>,
    pub app: Option<String>,
    pub query: Option<String>,
    pub severity_min: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsIncidentsArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
    pub window_minutes: Option<u32>,
    pub terms: Vec<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsInvestigateArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
    pub window_minutes: Option<u32>,
    pub correlation_window_minutes: Option<u32>,
    pub terms: Vec<String>,
    pub detail: SessionsOutputDetail,
    pub include_transcript: bool,
    pub max_bytes: Option<usize>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsAssessArgs {
    pub incident_id: String,
    pub model: Option<String>,
    pub json: bool,
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub window_minutes: Option<u32>,
    pub correlation_window_minutes: Option<u32>,
    pub terms: Vec<String>,
    pub limit: Option<u32>,
}
