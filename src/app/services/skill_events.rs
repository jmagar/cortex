//! `skill_events` read surface — lists already-extracted/backfilled
//! `ai_skill_events` rows. Extraction and persistence live in
//! `crate::scanner::skill_events` (ingest-time) and
//! `super::skill_backfill` (historical backfill); this module only wraps
//! `db::list_skill_events` with request/response model conversion, matching
//! the pattern used by sibling AI-inventory reads (e.g. `list_ai_tools`).

use crate::db;

use super::super::models::ListSkillEventsRequest;
use super::super::time::parse_optional_timestamp;
use super::{CortexService, ServiceResult};
use crate::app::models::ListSkillEventsResponse;

impl CortexService {
    pub async fn list_skill_events(
        &self,
        req: ListSkillEventsRequest,
    ) -> ServiceResult<ListSkillEventsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let params = db::AiSkillEventParams {
            skill: req.skill,
            plugin: req.plugin,
            tool: req.tool,
            project: req.project,
            session_id: req.session_id,
            hostname: req.hostname,
            from,
            to,
            limit: req.limit,
        };
        let result = self
            .run_db("list_skill_events", move |pool| {
                db::list_skill_events(pool, &params)
            })
            .await?;
        Ok(result.into())
    }
}
