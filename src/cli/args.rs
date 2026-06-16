use super::args_config::ConfigCommand;
use cortex::compose::{ComposeTarget, MutationOptions};

mod ai;
mod surface;

pub(crate) use ai::{
    AiAbuseArgs, AiAddArgs, AiAskHistoryArgs, AiAssessArgs, AiBlocksArgs, AiCheckpointsArgs,
    AiCommand, AiContextArgs, AiCorrelateArgs, AiDoctorArgs, AiErrorsArgs, AiIncidentContextArgs,
    AiIncidentsArgs, AiIndexArgs, AiInvestigateArgs, AiListArgs, AiOutputDetail,
    AiPruneCheckpointsArgs, AiSearchArgs, AiSimilarArgs, AiWatchArgs,
};
pub(crate) use surface::{
    AnomaliesArgs, AppsArgs, ClockSkewArgs, CompareArgs, CorrelateStateArgs, FleetStateArgs,
    HostStateArgs, SilentHostsArgs,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliCommand {
    Search(SearchArgs),
    Filter(FilterArgs),
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
    Inventory(InventoryCommand),
    SourceIps(SourceIpsArgs),
    Timeline(TimelineArgs),
    Patterns(PatternsArgs),
    IngestRate(IngestRateArgs),
    Sig(SigCommand),
    Notify(NotifyCommand),
    Shell(ShellCommand),
    AgentCommand(AgentCommandCommand),
    Heartbeat(HeartbeatCommand),
    SilentHosts(SilentHostsArgs),
    ClockSkew(ClockSkewArgs),
    Anomalies(AnomaliesArgs),
    Compare(CompareArgs),
    Apps(AppsArgs),
    HostState(HostStateArgs),
    FleetState(FleetStateArgs),
    CorrelateState(CorrelateStateArgs),
    Entity(EntityArgs),
    Graph(GraphCommand),
    FileTail(FileTailCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileTailCommand {
    List(FileTailListArgs),
    Status(FileTailListArgs),
    Add(FileTailAddArgs),
    Remove(FileTailIdArgs),
    Enable(FileTailIdArgs),
    Disable(FileTailIdArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FileTailListArgs {
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileTailIdArgs {
    pub id: String,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileTailAddArgs {
    pub id: String,
    pub path: String,
    pub tag: String,
    pub hostname: Option<String>,
    pub facility: Option<String>,
    pub severity: Option<String>,
    pub start_at_end: bool,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphCommand {
    Around(GraphAroundArgs),
    Explain(GraphExplainArgs),
    Evidence(GraphEvidenceArgs),
    Status(GraphStatusArgs),
    Rebuild(GraphRebuildArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InventoryCommand {
    Refresh(InventoryArgs),
    Status(InventoryArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InventoryArgs {
    pub json: bool,
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
    AtuinIndex(ShellAtuinIndexArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentCommandCommand {
    IngestSpool(AgentCommandIngestSpoolArgs),
    Wrap(AgentCommandWrapArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HeartbeatCommand {
    Agent(HeartbeatAgentArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellIndexArgs {
    pub path: String,
    pub shell: String,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellAtuinIndexArgs {
    pub path: String,
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
pub(crate) struct HeartbeatAgentArgs {
    pub target: Option<String>,
    pub token: Option<String>,
    pub interval_secs: u64,
    pub probe_deadline_ms: u64,
    pub collection_deadline_ms: u64,
    pub retry_buffer: usize,
    pub once: bool,
    pub emit: bool,
    pub json: bool,
    pub host_id_path: Option<String>,
    pub docker: bool,
    pub docker_url: Option<String>,
    pub journald: bool,
    pub syslog_target: Option<String>,
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
    /// Copy this binary into ~/.local/bin for terminal use.
    Install(SetupArgs),
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
    IntegrityStatus(DbIntegrityStatusArgs),
    Checkpoint(DbCheckpointArgs),
    Vacuum(DbVacuumArgs),
    Backup(DbBackupArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DbIntegrityArgs {
    pub quick: bool,
    pub json: bool,
    /// Run the check as a non-blocking server-side job (HTTP mode only). Returns
    /// a job id immediately; poll with `db integrity status <id>`.
    pub background: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DbIntegrityStatusArgs {
    pub job_id: i64,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OutputArgs {
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DbStatusArgs {
    pub json: bool,
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
    /// Literal substring text (FTS5-safe); mutually exclusive with `query`.
    pub grep: Option<String>,
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
pub(crate) struct FilterArgs {
    pub hostname: Option<String>,
    pub source_ip: Option<String>,
    pub severity: Option<String>,
    pub app_name: Option<String>,
    pub facility: Option<String>,
    pub exclude_facility: Option<String>,
    pub process_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub received_from: Option<String>,
    pub received_to: Option<String>,
    pub limit: Option<u32>,
    pub source_kind: Option<String>,
    pub tool: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub container: Option<String>,
    pub docker_host: Option<String>,
    pub stream: Option<String>,
    pub event_action: Option<String>,
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
pub(crate) struct EntityArgs {
    pub entity_type: Option<String>,
    pub key: Option<String>,
    pub alias_type: Option<String>,
    pub alias_key: Option<String>,
    pub limit: Option<u32>,
    pub evidence_sample_limit: Option<u32>,
    pub payload_budget: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GraphAroundArgs {
    pub entity_id: Option<i64>,
    pub entity_type: Option<String>,
    pub key: Option<String>,
    pub alias_type: Option<String>,
    pub alias_key: Option<String>,
    pub depth: Option<u32>,
    pub limit: Option<u32>,
    pub evidence_sample_limit: Option<u32>,
    pub payload_budget: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GraphExplainArgs {
    pub entity_id: Option<i64>,
    pub entity_type: Option<String>,
    pub key: Option<String>,
    pub alias_type: Option<String>,
    pub alias_key: Option<String>,
    pub depth: Option<u32>,
    pub beam_width: Option<u32>,
    pub max_chains: Option<u32>,
    pub evidence_sample_limit: Option<u32>,
    pub payload_budget: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GraphEvidenceArgs {
    pub evidence_id: i64,
    pub payload_budget: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GraphStatusArgs {
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GraphRebuildArgs {
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
