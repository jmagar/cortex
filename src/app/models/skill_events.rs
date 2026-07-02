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
    pub truncated: bool,
    pub dry_run: bool,
}
