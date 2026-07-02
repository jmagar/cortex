use serde_json::Value;

use crate::app::{AiSkillIncidentRequest, AiSkillInvestigateRequest};

use super::super::AppState;
use super::action_payload;

pub(super) async fn tool_skill_incidents(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AiSkillIncidentRequest = action_payload(args, "skill_incidents")?;
    let response = state.service.list_ai_skill_incidents(req).await?;
    tracing::debug!(
        incident_count = response.incidents.len(),
        total = response.total_incidents,
        "skill_incidents completed"
    );
    Ok(serde_json::to_value(response)?)
}

pub(super) async fn tool_skill_investigate(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AiSkillInvestigateRequest = action_payload(args, "skill_investigate")?;
    let response = state.service.investigate_ai_skill_incidents(req).await?;
    tracing::debug!(
        evidence_count = response.evidence.len(),
        total_incidents = response.total_incidents,
        no_data = response.no_data,
        "skill_investigate completed"
    );
    Ok(serde_json::to_value(response)?)
}
