use super::args_config::ConfigCommand;
use syslog_mcp::compose::{ComposeTarget, MutationOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliCommand {
    Search(SearchArgs),
    Tail(TailArgs),
    Errors(TimeRangeArgs),
    Hosts(OutputArgs),
    Sessions(SessionsArgs),
    Incident(IncidentArgs),
    Ai(AiCommand),
    Correlate(CorrelateArgs),
    Stats(OutputArgs),
    Compose(ComposeCommand),
    Service(ServiceCommand),
    Setup(SetupCommand),
    Db(DbCommand),
    Config(ConfigCommand),
    SourceIps(SourceIpsArgs),
    Timeline(TimelineArgs),
    Patterns(PatternsArgs),
    IngestRate(IngestRateArgs),
    Sig(SigCommand),
    Notify(NotifyCommand),
    Shell(ShellCommand),
    AgentCommand(AgentCommandCommand),
    // ── Surface parity gap closure (2026-05-22) ─────────────────────────────
    SilentHosts(SilentHostsArgs),
    ClockSkew(ClockSkewArgs),
    Anomalies(AnomaliesArgs),
    Compare(CompareArgs),
    Apps(AppsArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SigCommand {
    List(SigListArgs),
    Ack(SigAckArgs),
    Unack(SigUnackArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NotifyCommand {
    Recent(NotifyRecentArgs),
    Test(NotifyTestArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShellCommand {
    Index(ShellIndexArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentCommandCommand {
    IngestSpool(AgentCommandIngestSpoolArgs),
    Wrap(AgentCommandWrapArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellIndexArgs {
    pub path: String,
    pub shell: String,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentCommandIngestSpoolArgs {
    pub path: String,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentCommandWrapArgs {
    pub spool: String,
    pub command: Vec<String>,
}

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
    WatchStatus(OutputArgs),
    SmokeWatch(OutputArgs),
    SimilarIncidents(AiSimilarArgs),
    AskHistory(AiAskHistoryArgs),
    IncidentContext(AiIncidentContextArgs),
    Incidents(AiIncidentsArgs),
    Investigate(AiInvestigateArgs),
    Assess(AiAssessArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposeCommand {
    Status(ComposeArgs),
    Doctor(ComposeArgs),
    Up(ComposeMutationArgs),
    Down(ComposeMutationArgs),
    Restart(ComposeMutationArgs),
    Pull(ComposeMutationArgs),
    Logs(ComposeLogsArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServiceCommand {
    Logs(ServiceLogsArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetupCommand {
    Check(SetupArgs),
    Repair(SetupArgs),
    PluginHook(PluginHookArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SetupArgs {
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PluginHookArgs {
    pub json: bool,
    pub no_repair: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DbCommand {
    Status(DbStatusArgs),
    Integrity(DbIntegrityArgs),
    Checkpoint(DbCheckpointArgs),
    Vacuum(DbVacuumArgs),
    Backup(DbBackupArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DbIntegrityArgs {
    pub quick: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OutputArgs {
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DbStatusArgs {
    pub json: bool,
    /// Opt-in: run the host/container coordination diagnostic phases
    /// (`data-mount`, `ai-watch-coord`). These shell out to `docker inspect`
    /// and `systemctl --user show` and add roughly 100-200ms per invocation,
    /// so the default `db status` path is left untouched.
    pub check_coord: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DbCheckpointArgs {
    pub mode: String,
    pub json: bool,
}

impl Default for DbCheckpointArgs {
    fn default() -> Self {
        Self {
            mode: "passive".into(),
            json: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DbVacuumArgs {
    pub full: bool,
    pub pages: u32,
    /// CLI bool maps to server `Option<bool>` as
    /// `present → Some(true)`, `absent → None`. The size pre-flight on
    /// `--full` is bypassed only when the server receives `Some(true)`.
    /// See [`crate::app::models::DbVacuumRequest`] (bead 0p8r.4 #C3).
    pub force: bool,
    pub json: bool,
}

impl Default for DbVacuumArgs {
    fn default() -> Self {
        Self {
            full: false,
            pages: 1000,
            force: false,
            json: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DbBackupArgs {
    pub output: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiDoctorArgs {
    pub json: bool,
    pub strict_permissions: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ComposeArgs {
    pub target: ComposeTarget,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ComposeMutationArgs {
    pub target: ComposeTarget,
    pub options: MutationOptions,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ComposeLogsArgs {
    pub target: ComposeTarget,
    pub tail: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceLogsArgs {
    pub service: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub tail: Option<u32>,
    pub json: bool,
}

impl Default for ServiceLogsArgs {
    fn default() -> Self {
        Self {
            service: String::new(),
            from: None,
            to: None,
            tail: Some(200),
            json: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IncidentArgs {
    pub around: String,
    pub minutes: Option<u32>,
    pub service: Option<String>,
    pub hostname: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionsArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub hostname: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SearchArgs {
    pub query: Option<String>,
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub severity: Option<String>,
    pub app_name: Option<String>,
    pub facility: Option<String>,
    pub exclude_facility: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub received_from: Option<String>,
    pub received_to: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TailArgs {
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub app_name: Option<String>,
    pub n: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TimeRangeArgs {
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CorrelateArgs {
    pub reference_time: String,
    pub window_minutes: Option<u32>,
    pub severity_min: Option<String>,
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub query: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiSearchArgs {
    pub query: String,
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiAbuseArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
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
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub app_name: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
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
    pub from: Option<String>,
    pub to: Option<String>,
    pub json: bool,
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
    pub from: Option<String>,
    pub to: Option<String>,
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
    pub hostname: Option<String>,
    pub app_name: Option<String>,
    pub severity_min: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub window_minutes: Option<u32>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiAskHistoryArgs {
    pub query: String,
    pub hostname: Option<String>,
    pub app_name: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiIncidentContextArgs {
    pub from: String,
    pub to: String,
    pub hostname: Option<String>,
    pub app_name: Option<String>,
    pub query: Option<String>,
    pub severity_min: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SourceIpsArgs {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TimelineArgs {
    pub bucket: Option<String>,
    pub group_by: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub hostname: Option<String>,
    pub app_name: Option<String>,
    pub severity_min: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PatternsArgs {
    pub from: Option<String>,
    pub to: Option<String>,
    pub hostname: Option<String>,
    pub app_name: Option<String>,
    pub severity_min: Option<String>,
    pub scan_limit: Option<u32>,
    pub top_n: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IngestRateArgs {
    pub by_host: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SigListArgs {
    pub limit: Option<u32>,
    pub include_acknowledged: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SigAckArgs {
    pub signature_hash: String,
    pub notes: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SigUnackArgs {
    pub signature_hash: String,
    pub reason: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NotifyRecentArgs {
    pub limit: Option<i64>,
    pub rule_id: Option<String>,
    pub since: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NotifyTestArgs {
    pub body: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiIncidentsArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub window_minutes: Option<u32>,
    pub terms: Vec<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiInvestigateArgs {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub window_minutes: Option<u32>,
    pub correlation_window_minutes: Option<u32>,
    pub terms: Vec<String>,
    pub json: bool,
}

// ── Surface parity gap closure args (2026-05-22) ───────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SilentHostsArgs {
    pub silent_minutes: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ClockSkewArgs {
    pub since: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AnomaliesArgs {
    pub recent_minutes: Option<u32>,
    pub baseline_minutes: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CompareArgs {
    pub a_from: Option<String>,
    pub a_to: Option<String>,
    pub b_from: Option<String>,
    pub b_to: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AppsArgs {
    pub hostname: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AiAssessArgs {
    pub incident_id: String,
    pub model: Option<String>,
    pub json: bool,
    pub project: Option<String>,
    pub tool: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub window_minutes: Option<u32>,
    pub correlation_window_minutes: Option<u32>,
    pub terms: Vec<String>,
    pub limit: Option<u32>,
}
