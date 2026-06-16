use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageBlocksRequest {
    pub project: Option<String>,
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageBlock {
    pub bucket_start: String,
    pub bucket_end: String,
    pub project: String,
    pub tool: String,
    pub session_count: i64,
    pub event_count: i64,
}

impl From<db::AiUsageBlock> for UsageBlock {
    fn from(value: db::AiUsageBlock) -> Self {
        Self {
            bucket_start: value.bucket_start,
            bucket_end: value.bucket_end,
            project: value.project,
            tool: value.tool,
            session_count: value.session_count,
            event_count: value.event_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageBlocksResponse {
    pub total_blocks: usize,
    pub truncated: bool,
    pub blocks: Vec<UsageBlock>,
}

impl From<db::AiUsageBlocksResult> for UsageBlocksResponse {
    fn from(value: db::AiUsageBlocksResult) -> Self {
        Self {
            total_blocks: value.total_blocks,
            truncated: value.truncated,
            blocks: value.blocks.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectContextRequest {
    pub project: String,
    pub tool: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContextResponse {
    pub project: String,
    pub tools: Vec<String>,
    pub sessions: Vec<String>,
    pub hostnames: Vec<String>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    pub event_count: i64,
    pub recent_entries_truncated: bool,
    pub recent_entries: Vec<LogEntry>,
}

impl From<db::AiProjectContext> for ProjectContextResponse {
    fn from(value: db::AiProjectContext) -> Self {
        Self {
            project: value.project,
            tools: value.tools,
            sessions: value.sessions,
            hostnames: value.hostnames,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
            event_count: value.event_count,
            recent_entries_truncated: value.recent_entries_truncated,
            recent_entries: value.recent_entries.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListAiToolsRequest {
    pub project: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolEntry {
    pub tool: String,
    pub event_count: i64,
    pub session_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

impl From<db::AiToolInventoryEntry> for AiToolEntry {
    fn from(value: db::AiToolInventoryEntry) -> Self {
        Self {
            tool: value.tool,
            event_count: value.event_count,
            session_count: value.session_count,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAiToolsResponse {
    pub total_tools: usize,
    pub truncated: bool,
    pub tools: Vec<AiToolEntry>,
}

impl From<db::ListAiToolsResult> for ListAiToolsResponse {
    fn from(value: db::ListAiToolsResult) -> Self {
        Self {
            total_tools: value.total_tools,
            truncated: value.truncated,
            tools: value.tools.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListAiProjectsRequest {
    pub tool: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProjectEntry {
    pub project: String,
    pub tools: Vec<String>,
    pub event_count: i64,
    pub session_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

impl From<db::AiProjectInventoryEntry> for AiProjectEntry {
    fn from(value: db::AiProjectInventoryEntry) -> Self {
        Self {
            project: value.project,
            tools: value.tools,
            event_count: value.event_count,
            session_count: value.session_count,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAiProjectsResponse {
    pub total_projects: usize,
    pub truncated: bool,
    pub projects: Vec<AiProjectEntry>,
}

impl From<db::ListAiProjectsResult> for ListAiProjectsResponse {
    fn from(value: db::ListAiProjectsResult) -> Self {
        Self {
            total_projects: value.total_projects,
            truncated: value.truncated,
            projects: value.projects.into_iter().map(Into::into).collect(),
        }
    }
}
