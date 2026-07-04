use serde_json::Value;

use crate::app::{AiMcpIncidentRequest, AiMcpInvestigateRequest};

use super::super::AppState;
use super::action_payload;

pub(super) async fn tool_mcp_incidents(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AiMcpIncidentRequest = action_payload(args, "mcp_incidents")?;
    let response = state.service.list_ai_mcp_incidents(req).await?;
    tracing::debug!(
        incident_count = response.incidents.len(),
        total = response.total_incidents,
        "mcp_incidents completed"
    );
    Ok(serde_json::to_value(response)?)
}

pub(super) async fn tool_mcp_investigate(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AiMcpInvestigateRequest = action_payload(args, "mcp_investigate")?;
    let response = state.service.investigate_ai_mcp_incidents(req).await?;
    tracing::debug!(
        evidence_count = response.evidence.len(),
        total_incidents = response.total_incidents,
        no_data = response.no_data,
        "mcp_investigate completed"
    );
    Ok(serde_json::to_value(response)?)
}
