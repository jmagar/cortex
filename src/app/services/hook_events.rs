//! `hook_events` read surface — lists already-extracted/backfilled/collected
//! `ai_hook_events` rows. Runtime extraction lives in
//! `crate::scanner::hook_events` (ingest-time) and `super::hook_backfill`
//! (historical backfill); config-inventory collection lives in
//! `crate::hook_config`. This module only wraps `db::list_hook_events` with
//! request/response model conversion, mirroring `super::skill_events`.

use crate::db;

use super::super::models::ListHookEventsRequest;
use super::super::time::parse_optional_timestamp;
use super::{CortexService, ServiceResult};
use crate::app::models::ListHookEventsResponse;

impl CortexService {
    pub async fn list_hook_events(
        &self,
        req: ListHookEventsRequest,
    ) -> ServiceResult<ListHookEventsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let params = db::AiHookEventParams {
            hook_event: req.hook_event,
            hook_name: req.hook_name,
            hook_source: req.hook_source,
            status: req.status,
            evidence_kind: req.evidence_kind,
            tool: req.tool,
            project: req.project,
            session_id: req.session_id,
            hostname: req.hostname,
            from,
            to,
            limit: req.limit,
        };
        let result = self
            .run_db("list_hook_events", move |pool| {
                db::list_hook_events(pool, &params)
            })
            .await?;
        Ok(result.into())
    }
}
