//! `mcp_events` read surface — lists already-extracted/backfilled
//! `ai_mcp_events` rows. Extraction and persistence live in
//! `crate::scanner::mcp_events` (ingest-time) and `super::mcp_backfill`
//! (historical backfill); this module only wraps `db::list_mcp_events` with
//! request/response model conversion, matching the pattern used by
//! `super::skill_events`.

use crate::db;

use super::super::models::ListMcpEventsRequest;
use super::super::time::parse_optional_timestamp;
use super::{CortexService, ServiceResult};
use crate::app::models::ListMcpEventsResponse;

impl CortexService {
    pub async fn list_mcp_events(
        &self,
        req: ListMcpEventsRequest,
    ) -> ServiceResult<ListMcpEventsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let params = db::AiMcpEventParams {
            tool_name: req.tool_name,
            mcp_server: req.mcp_server,
            mcp_tool: req.mcp_tool,
            ai_tool: req.tool,
            ai_project: req.project,
            ai_session_id: req.session_id,
            hostname: req.hostname,
            is_error: req.is_error,
            from,
            to,
            limit: req.limit,
        };
        let result = self
            .run_db("list_mcp_events", move |pool| {
                db::list_mcp_events(pool, &params)
            })
            .await?;
        Ok(result.into())
    }
}
