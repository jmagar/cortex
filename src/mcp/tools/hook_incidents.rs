use serde_json::Value;

use crate::app::{AiHookIncidentRequest, AiHookInvestigateRequest};

use super::super::AppState;
use super::action_payload;

pub(super) async fn tool_hook_incidents(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AiHookIncidentRequest = action_payload(args, "hook_incidents")?;
    let response = state.service.list_ai_hook_incidents(req).await?;
    tracing::debug!(
        incident_count = response.incidents.len(),
        total = response.total_incidents,
        "hook_incidents completed"
    );
    Ok(serde_json::to_value(response)?)
}

pub(super) async fn tool_hook_investigate(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AiHookInvestigateRequest = action_payload(args, "hook_investigate")?;
    let response = state.service.investigate_ai_hook_incidents(req).await?;
    tracing::debug!(
        evidence_count = response.evidence.len(),
        total_incidents = response.total_incidents,
        no_data = response.no_data,
        "hook_investigate completed"
    );
    Ok(serde_json::to_value(response)?)
}
