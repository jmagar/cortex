use serde_json::Value;

use crate::app::{
    AskHistoryRequest, GraphAroundRequest, GraphEntityLookupRequest, GraphEvidenceLookupRequest,
    GraphExplainRequest, IncidentContextRequest, SimilarIncidentsRequest,
};

use super::super::AppState;
use super::{action_payload, string_arg};

pub(super) async fn tool_similar_incidents(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: SimilarIncidentsRequest = action_payload(args, "similar_incidents")?;
    let response = state.service.analysis().similar_incidents(req).await?;
    tracing::debug!(
        cluster_count = response.total_clusters,
        "similar_incidents completed"
    );
    Ok(serde_json::to_value(response)?)
}

pub(super) async fn tool_ask_history(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: AskHistoryRequest = action_payload(args, "ask_history")?;
    let response = state.service.ask_history(req).await?;
    tracing::debug!(
        session_count = response.sessions.len(),
        "ask_history completed"
    );
    Ok(serde_json::to_value(response)?)
}

pub(super) async fn tool_incident_context(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let req: IncidentContextRequest = action_payload(args, "incident_context")?;
    let response = state.service.analysis().incident_context(req).await?;
    tracing::debug!(
        total_logs = response.total_logs,
        error_count = response.error_logs.len(),
        "incident_context completed"
    );
    Ok(serde_json::to_value(response)?)
}

pub(super) async fn tool_graph(state: &AppState, args: Value) -> anyhow::Result<Value> {
    let mode = string_arg(&args, "mode").unwrap_or_else(|| "around".to_string());
    match mode.as_str() {
        "entity" => {
            let req: GraphEntityLookupRequest = action_payload(args, "graph")?;
            Ok(serde_json::to_value(
                state.service.graph_entity_lookup(req).await?,
            )?)
        }
        "around" => {
            let req: GraphAroundRequest = action_payload(args, "graph")?;
            Ok(serde_json::to_value(
                state.service.graph_around(req).await?,
            )?)
        }
        "explain" => {
            let req: GraphExplainRequest = action_payload(args, "graph")?;
            Ok(serde_json::to_value(
                state.service.graph_explain(req).await?,
            )?)
        }
        "evidence" => {
            let req: GraphEvidenceLookupRequest = action_payload(args, "graph")?;
            Ok(serde_json::to_value(
                state.service.graph_evidence_lookup(req).await?,
            )?)
        }
        other => Err(anyhow::anyhow!(
            "unsupported graph mode '{other}'; expected entity, around, explain, or evidence"
        )),
    }
}
