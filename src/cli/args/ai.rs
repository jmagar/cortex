#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AiCommand {
    Search(AiSearchArgs),
    Abuse(AiAbuseArgs),
    Correlate(AiCorrelateArgs),
    Blocks(AiBlocksArgs),
    Context(AiContextArgs),
    Tools(AiListArgs),
    Projects(AiListArgs),
    Index(AiIndexArgs),
    Add(AiAddArgs),
    Watch(AiWatchArgs),
    Checkpoints(AiCheckpointsArgs),
    Errors(AiErrorsArgs),
    PruneCheckpoints(AiPruneCheckpointsArgs),
    Doctor(AiDoctorArgs),
    WatchStatus(super::OutputArgs),
    SmokeWatch(super::OutputArgs),
    SimilarIncidents(AiSimilarArgs),
    AskHistory(AiAskHistoryArgs),
    IncidentContext(AiIncidentContextArgs),
    Incidents(AiIncidentsArgs),
    Investigate(AiInvestigateArgs),
    Assess(AiAssessArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiDoctorArgs {
    pub json: bool,
    pub strict_permissions: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiSearchArgs {
    pub query: String,
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiAbuseArgs {
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
pub(crate) struct AiCorrelateArgs {
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
pub(crate) struct AiBlocksArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<usize>,
    pub detail: AiOutputDetail,
    pub json: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum AiOutputDetail {
    #[default]
    Compact,
    Full,
}

impl AiOutputDetail {
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
pub(crate) struct AiContextArgs {
    pub project: String,
    pub tool: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiListArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiIndexArgs {
    pub path: Option<String>,
    pub force: bool,
    pub since: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiAddArgs {
    pub file: String,
    pub force: bool,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AiWatchArgs {
    pub path: Option<String>,
    pub debounce_ms: u64,
    pub settle_ms: u64,
    pub max_retries: u8,
    pub no_initial_scan: bool,
    pub json: bool,
}

impl Default for AiWatchArgs {
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
pub(crate) struct AiCheckpointsArgs {
    pub errors_only: bool,
    pub missing_only: bool,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiErrorsArgs {
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiPruneCheckpointsArgs {
    pub missing_only: bool,
    pub dry_run: bool,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiSimilarArgs {
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
pub(crate) struct AiAskHistoryArgs {
    pub query: String,
    pub host: Option<String>,
    pub app: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiIncidentContextArgs {
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
pub(crate) struct AiIncidentsArgs {
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
pub(crate) struct AiInvestigateArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
    pub window_minutes: Option<u32>,
    pub correlation_window_minutes: Option<u32>,
    pub terms: Vec<String>,
    pub detail: AiOutputDetail,
    pub include_transcript: bool,
    pub max_bytes: Option<usize>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiAssessArgs {
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
