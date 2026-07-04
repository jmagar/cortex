use lab_auth::AuthContext;
use serde_json::Value;

use crate::app::{AckErrorRequest, RequestActor, UnackErrorRequest};

use super::super::{AppState, AuthPolicy};
use super::{action_payload, string_arg};

/// Return a stable actor identifier for mutating/admin actions.
///
/// Mounted MCP requests carry caller identity in `AuthContext`. Prefer the
/// verified email when available, then the subject. Loopback mode has no
/// per-request credential, so it falls back to the local trust-boundary actor.
fn extract_actor(state: &AppState, auth: Option<&AuthContext>) -> RequestActor {
    if let Some(auth) = auth {
        return RequestActor::mcp_identity(
            (!auth.sub.is_empty()).then(|| auth.sub.clone()),
            auth.email
                .as_deref()
                .filter(|email| !email.is_empty())
                .map(str::to_string),
        );
    }

    match &state.auth_policy {
        AuthPolicy::LoopbackDev => RequestActor::mcp_loopback(),
        AuthPolicy::TrustedGatewayUnscoped => "mcp:trusted-gateway".to_string().into(),
        AuthPolicy::Mounted {
            auth_state: Some(_),
        } => RequestActor::mcp_oauth(),
        AuthPolicy::Mounted { auth_state: None } => RequestActor::mcp_bearer(),
    }
}

pub(super) async fn tool_ack_error(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    let req: AckErrorRequest = action_payload(args, "ack_error")?;
    let actor = extract_actor(state, auth);
    let resp = state.service.alerts().ack_signature(req, actor).await?;
    Ok(serde_json::to_value(resp)?)
}

pub(super) async fn tool_unack_error(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    let req: UnackErrorRequest = action_payload(args, "unack_error")?;
    let actor = extract_actor(state, auth);
    let resp = state.service.alerts().unack_signature(req, actor).await?;
    Ok(serde_json::to_value(resp)?)
}

pub(super) async fn tool_notifications_test(
    state: &AppState,
    args: Value,
    auth: Option<&AuthContext>,
) -> anyhow::Result<Value> {
    let body =
        string_arg(&args, "body").unwrap_or_else(|| "Test notification from cortex".to_string());
    // Actor is derived from request auth context, not caller-supplied args.
    let actor = extract_actor(state, auth);
    let result = state
        .service
        .alerts()
        .test_notification(body, actor, &state.notifications_config)
        .await?;
    Ok(serde_json::json!({ "result": result }))
}
