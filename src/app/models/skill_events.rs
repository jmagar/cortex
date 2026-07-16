use super::*;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillBackfillRequest {
    pub since: Option<String>,
    pub limit: Option<u64>,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillBackfillResult {
    pub scanned: u64,
    pub inserted: u64,
    pub skipped_duplicates: u64,
    pub parse_errors: u64,
    /// Claude rows whose raw JSON could not be recovered from the source
    /// transcript file — `ai_transcript_path`/`line_no` metadata missing, the
    /// source file no longer exists, or the recorded line number is out of
    /// range (e.g. file rotated/truncated since ingest). Distinct from
    /// `parse_errors`, which counts lines that WERE located but failed to
    /// parse as JSON.
    pub source_unavailable: u64,
    pub truncated: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListSkillEventsRequest {
    pub skill: Option<String>,
    pub plugin: Option<String>,
    pub tool: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub hostname: Option<String>,
    #[serde(alias = "since")]
    pub from: Option<String>,
    #[serde(alias = "until")]
    pub to: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEventEntry {
    pub id: i64,
    pub log_id: i64,
    pub ai_tool: String,
    pub ai_project: Option<String>,
    pub ai_session_id: Option<String>,
    pub hostname: String,
    pub timestamp: String,
    pub skill_name: String,
    pub skill_plugin: Option<String>,
    pub event_kind: String,
    pub evidence_kind: String,
}

impl From<db::AiSkillEventEntry> for SkillEventEntry {
    fn from(value: db::AiSkillEventEntry) -> Self {
        Self {
            id: value.id,
            log_id: value.log_id,
            ai_tool: value.ai_tool,
            ai_project: value.ai_project,
            ai_session_id: value.ai_session_id,
            hostname: value.hostname,
            timestamp: value.timestamp,
            skill_name: value.skill_name,
            skill_plugin: value.skill_plugin,
            event_kind: value.event_kind,
            evidence_kind: value.evidence_kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSkillEventsResponse {
    pub total: usize,
    pub truncated: bool,
    pub events: Vec<SkillEventEntry>,
}

impl From<db::ListSkillEventsResult> for ListSkillEventsResponse {
    fn from(value: db::ListSkillEventsResult) -> Self {
        Self {
            total: value.total,
            truncated: value.truncated,
            events: value.events.into_iter().map(Into::into).collect(),
        }
    }
}
