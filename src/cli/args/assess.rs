//! `cortex assess` — unified verb namespace for LLM-guarded and
//! deterministic incident assessment. `Hooks` remains a minimal stub
//! tracked by GH #105 — do not add real hooks logic here. `Mcp` is now
//! implemented (GH #104).

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssessCommand {
    Skill(AssessSkillArgs),
    Abuse(AssessAbuseArgs),
    Mcp(AssessMcpArgs),
    /// Stub — replaced by the `hooks` phase's own args type + parse
    /// function (GH #105).
    #[allow(dead_code)]
    Hooks(Vec<String>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AssessSkillArgs {
    pub skill: Option<String>,
    pub plugin: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub window_minutes: Option<u32>,
    pub correlation_window_minutes: Option<u32>,
    pub limit: Option<u32>,
    pub all: bool,
    pub no_llm: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AssessAbuseArgs {
    pub incident_id: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub window_minutes: Option<u32>,
    pub correlation_window_minutes: Option<u32>,
    pub limit: Option<u32>,
    pub no_llm: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AssessMcpArgs {
    /// Bare positional argument — an mcp_server, mcp_tool, or raw tool
    /// name. `None` when the caller used `--server`/`--tool` flags only.
    pub target: Option<String>,
    pub server: Option<String>,
    pub tool_name: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub window_minutes: Option<u32>,
    pub correlation_window_minutes: Option<u32>,
    pub limit: Option<u32>,
    pub all: bool,
    pub no_llm: bool,
    pub json: bool,
}
