use std::{borrow::Cow, net::Ipv6Addr, sync::Arc, time::Instant};

use lab_auth::AuthContext;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, Content, Implementation, ListResourcesResult,
        ListToolsResult, Meta, PaginatedRequestParams, RawResource, ReadResourceRequestParams,
        ReadResourceResult, Resource, ResourceContents, ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    ErrorData, RoleServer, ServerHandler,
};
use serde_json::{Map, Value};

use crate::app::ServiceError;
use crate::config::McpConfig;

use super::actions;
use super::{schemas::tool_definitions, tools::execute_tool, AppState, AuthPolicy};

#[derive(Clone)]
pub struct SyslogRmcpServer {
    state: AppState,
}

impl SyslogRmcpServer {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

pub fn rmcp_server(state: AppState) -> SyslogRmcpServer {
    SyslogRmcpServer::new(state)
}

impl ServerHandler for SyslogRmcpServer {
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        // tools/list requires AuthContext when policy is Mounted (but no scope).
        // LoopbackDev bypasses the check entirely.
        require_auth_context(&self.state, &context)?;

        let tools = rmcp_tool_definitions()?;
        tracing::info!(tool_count = tools.len(), "MCP tools listed");
        Ok(ListToolsResult {
            tools,
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let tool_name = request.name.to_string();

        // Extract action for scope check before any DB work fires.
        // Clone into an owned String so request.arguments can be consumed below.
        let action: String = request
            .arguments
            .as_ref()
            .and_then(|m| m.get("action"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();

        // Fail-closed auth check: require AuthContext when Mounted, then scope.
        // LoopbackDev returns None — no scope enforcement applies.
        let auth = require_auth_context(&self.state, &context)?;
        if let (Some(auth), Some(required_scope)) = (auth, required_scope_for(&action)) {
            check_scope(auth, required_scope, &action)?;
        }

        let arguments = request
            .arguments
            .map(Value::Object)
            .unwrap_or_else(|| Value::Object(Map::new()));
        let started = Instant::now();
        tracing::info!(tool = %tool_name, "MCP tool execution started");

        match execute_tool(&self.state, &tool_name, arguments, auth).await {
            Ok(result) => {
                let result_count = safe_result_count(&result);
                tracing::info!(
                    tool = %tool_name,
                    elapsed_ms = started.elapsed().as_millis(),
                    result_count,
                    "MCP tool execution completed"
                );
                tool_result_from_json(result)
            }
            Err(error) if is_validation_error(&error) => {
                tracing::warn!(
                    tool = %tool_name,
                    elapsed_ms = started.elapsed().as_millis(),
                    error_class = "invalid_params",
                    "MCP tool execution rejected invalid params"
                );
                Err(ErrorData::invalid_params(error.to_string(), None))
            }
            Err(error) => {
                tracing::error!(
                    tool = %tool_name,
                    elapsed_ms = started.elapsed().as_millis(),
                    error = %error,
                    error_class = "tool_execution",
                    "MCP tool execution failed"
                );
                Ok(CallToolResult::error(vec![Content::text(format!(
                    "Tool execution failed for action '{action}'. Check server logs for details."
                ))]))
            }
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        require_auth_context(&self.state, &context)?;
        Ok(ListResourcesResult {
            resources: vec![schema_resource(), query_widget_resource()],
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        require_auth_context(&self.state, &context)?;
        match request.uri.as_str() {
            SCHEMA_RESOURCE_URI => {
                let schema = tool_definitions();
                let text = serde_json::to_string_pretty(&schema).map_err(|error| {
                    ErrorData::internal_error(format!("serialization error: {error}"), None)
                })?;
                Ok(ReadResourceResult::new(vec![ResourceContents::text(
                    text,
                    SCHEMA_RESOURCE_URI,
                )
                .with_mime_type("application/json")]))
            }
            QUERY_WIDGET_RESOURCE_URI => Ok(ReadResourceResult::new(vec![query_widget_contents()])),
            _ => Err(ErrorData::invalid_params(
                format!("unknown resource: {}", request.uri),
                None,
            )),
        }
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            self.state.config.server_name.clone(),
            env!("CARGO_PKG_VERSION"),
        ))
    }
}

pub fn streamable_http_config(config: &McpConfig) -> StreamableHttpServerConfig {
    StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(allowed_hosts(config))
        .with_allowed_origins(allowed_origins(config))
}

pub fn streamable_http_service(
    state: AppState,
    config: StreamableHttpServerConfig,
) -> StreamableHttpService<SyslogRmcpServer, LocalSessionManager> {
    StreamableHttpService::new(
        move || Ok(SyslogRmcpServer::new(state.clone())),
        Default::default(),
        config,
    )
}

const SCHEMA_RESOURCE_URI: &str = "syslog://schema/mcp-tool";
pub(super) const QUERY_WIDGET_RESOURCE_URI: &str = "ui://syslog/query-widget";
pub(super) const MCP_APP_HTML_MIME_TYPE: &str = "text/html;profile=mcp-app";

fn schema_resource() -> Resource {
    Resource::new(
        RawResource::new(SCHEMA_RESOURCE_URI, "syslog tool schema")
            .with_description("JSON schema for the syslog MCP tool and its action-based parameters")
            .with_mime_type("application/json"),
        None,
    )
}

fn query_widget_resource() -> Resource {
    Resource::new(
        RawResource::new(QUERY_WIDGET_RESOURCE_URI, "syslog query widget")
            .with_title("Syslog Query")
            .with_description("Interactive MCP Apps widget for querying syslog-mcp logs")
            .with_mime_type(MCP_APP_HTML_MIME_TYPE),
        None,
    )
}

fn query_widget_contents() -> ResourceContents {
    ResourceContents::text(
        include_str!("ui/query_widget.html"),
        QUERY_WIDGET_RESOURCE_URI,
    )
    .with_mime_type(MCP_APP_HTML_MIME_TYPE)
}

fn syslog_tool_meta() -> Meta {
    let mut meta = Map::new();
    meta.insert(
        "ui".to_string(),
        serde_json::json!({
            "resourceUri": QUERY_WIDGET_RESOURCE_URI,
            "visibility": ["model", "app"],
        }),
    );
    Meta(meta)
}

fn rmcp_tool_definitions() -> Result<Vec<Tool>, ErrorData> {
    tool_definitions()
        .into_iter()
        .map(rmcp_tool_from_json)
        .collect()
}

fn rmcp_tool_from_json(value: Value) -> Result<Tool, ErrorData> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ErrorData::internal_error("tool definition missing name", None))?;
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .map(|description| Cow::Owned(description.to_string()));
    let input_schema = value
        .get("inputSchema")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| ErrorData::internal_error("tool definition missing inputSchema", None))?;

    let tool = Tool::new_with_raw(
        Cow::Owned(name.to_string()),
        description,
        Arc::new(input_schema),
    );

    Ok(if name == "syslog" {
        tool.with_meta(syslog_tool_meta())
    } else {
        tool
    })
}

fn tool_result_from_json(value: Value) -> Result<CallToolResult, ErrorData> {
    let text = serde_json::to_string_pretty(&value).map_err(|error| {
        ErrorData::internal_error(format!("serialization error: {error}"), None)
    })?;
    let mut result = CallToolResult::structured(value);
    result.content = vec![Content::text(text)];
    Ok(result)
}

fn is_validation_error(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<ServiceError>(),
        Some(ServiceError::InvalidInput(_))
    ) || error.to_string().contains(" is required")
        || error.to_string().contains("must be an unsigned integer")
        || error.to_string().contains(" must be <=")
        || error.to_string().contains("target override")
        || error.to_string().contains("unknown syslog action")
}

fn safe_result_count(value: &Value) -> Option<usize> {
    value
        .get("count")
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .or_else(|| value.get("hosts").and_then(Value::as_array).map(Vec::len))
        .or_else(|| value.get("summary").and_then(Value::as_array).map(Vec::len))
}

// ── Auth helpers ─────────────────────────────────────────────────────────────

/// Extract and enforce the authentication context from the rmcp request.
///
/// - `AuthPolicy::LoopbackDev`: always returns `Ok(None)` — the loopback bind
///   is the trust boundary; no per-request credential needed.
/// - `AuthPolicy::Mounted(_)`: the middleware MUST have inserted an
///   [`AuthContext`] into the request extensions. If it is absent, this
///   returns a forbidden error immediately (fail-closed).
///
/// Returns `Ok(Some(&AuthContext))` for Mounted+present, `Ok(None)` for
/// LoopbackDev. Callers can skip the scope check when the result is `None`.
fn require_auth_context<'a>(
    state: &AppState,
    ctx: &'a RequestContext<RoleServer>,
) -> Result<Option<&'a AuthContext>, ErrorData> {
    match &state.auth_policy {
        AuthPolicy::LoopbackDev => Ok(None),
        AuthPolicy::Mounted { .. } => {
            let parts = ctx
                .extensions
                .get::<axum::http::request::Parts>()
                .ok_or_else(|| {
                    // This indicates a framework-level invariant violation —
                    // rmcp changed how it propagates HTTP Parts, or the middleware
                    // ordering is broken at startup.
                    tracing::error!(
                        "rmcp HTTP Parts extension absent — middleware ordering may be broken; \
                         see docs/internal/rmcp-auth-spike.md"
                    );
                    ErrorData::invalid_request("forbidden: missing http context", None)
                })?;
            let auth = parts.extensions.get::<AuthContext>().ok_or_else(|| {
                // AuthLayer should always insert AuthContext on the happy path.
                // Absence here means AuthLayer is not mounted or failed to run.
                tracing::warn!(
                    "AuthContext absent from request extensions — \
                     AuthLayer may not be mounted or rejected the request without inserting context"
                );
                ErrorData::invalid_request("forbidden: missing auth context", None)
            })?;
            Ok(Some(auth))
        }
    }
}

/// Enforce that `auth` carries `required_scope`.
///
/// `syslog:admin` is treated as a superset of `syslog:read` — a caller with
/// admin access implicitly satisfies any read-level scope requirement.
///
/// Logs a warning with subject + action on denial (audit trail).
/// Only called when policy is Mounted (LoopbackDev short-circuits at the
/// caller via `require_auth_context` returning `None`).
fn check_scope(auth: &AuthContext, required_scope: &str, action: &str) -> Result<(), ErrorData> {
    let satisfied = auth
        .scopes
        .iter()
        .any(|s| s == required_scope || (required_scope == "syslog:read" && s == "syslog:admin"));
    if satisfied {
        return Ok(());
    }
    tracing::warn!(
        subject = %auth.sub,
        action = %action,
        required_scope = %required_scope,
        "MCP tool invocation denied: insufficient scope"
    );
    Err(ErrorData::invalid_request(
        format!("forbidden: requires scope: {required_scope}"),
        None,
    ))
}

/// Map a syslog tool action to the minimum required scope.
///
/// Map an action name to the MCP scope required to invoke it.
///
/// Delegates to [`actions::required_scope_for`] — the single authoritative
/// source. Kept as a local wrapper so call sites inside this module are
/// unchanged.
///
/// - `None` for `InfoOnly` actions (auth context required, no scope gate).
/// - `Some(READ_SCOPE)` / `Some(ADMIN_SCOPE)` for normal actions.
/// - `Some(DENY_SCOPE)` for unknown actions (fail-closed).
fn required_scope_for(action: &str) -> Option<&'static str> {
    actions::required_scope_for(action)
}

pub(super) fn allowed_hosts(config: &McpConfig) -> Vec<String> {
    let mut hosts = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    for host in &config.allowed_hosts {
        push_host_variants(&mut hosts, host, config.port);
    }
    push_host_variants(&mut hosts, &config.host, config.port);
    push_host_variants(&mut hosts, "localhost", config.port);
    push_host_variants(&mut hosts, "127.0.0.1", config.port);
    push_host_variants(&mut hosts, "::1", config.port);
    // When SYSLOG_MCP_PUBLIC_URL is set (required for OAuth mode), extract the
    // host and add it to the allowlist so callbacks from the public hostname are
    // accepted by rmcp's DNS-rebinding guard.
    if let Some(public_url) = config.auth.public_url.as_deref() {
        push_public_url_hosts(&mut hosts, public_url, config.port);
    }
    hosts.sort();
    hosts.dedup();
    hosts
}

fn push_host_variants(hosts: &mut Vec<String>, host: &str, port: u16) {
    let host = host.trim();
    if host.is_empty() {
        return;
    }
    hosts.push(host.to_string());
    if host.starts_with('[') && host.contains("]:") {
        return;
    }
    if let Some(inner) = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        if !inner.is_empty() {
            hosts.push(format!("[{inner}]:{port}"));
        }
    } else if host.parse::<Ipv6Addr>().is_ok() {
        hosts.push(format!("[{host}]"));
        hosts.push(format!("[{host}]:{port}"));
    } else if !has_port(host) {
        hosts.push(format!("{host}:{port}"));
    }
}

/// Extract the host (and explicit port, if any) from a URL string and push
/// variants into the hosts allowlist.
///
/// Browsers send the Host header with port only when it differs from the
/// default for the scheme (80 for http, 443 for https). We emit both the
/// bare host and `host:port` so that both direct (non-standard-port) and
/// reverse-proxied (standard-port) deployments are covered.
fn push_public_url_hosts(hosts: &mut Vec<String>, url: &str, listen_port: u16) {
    let Ok(parsed) = url::Url::parse(url) else {
        tracing::warn!(
            public_url = url,
            "SYSLOG_MCP_PUBLIC_URL is not a valid URL; skipping host allowlist extension"
        );
        return;
    };
    let Some(host) = parsed.host_str() else {
        return;
    };
    // Never add wildcards — rmcp does exact host matching and a wildcard in
    // the allowlist would silently permit any Host header value.
    if host.contains('*') {
        tracing::warn!(
            host,
            "SYSLOG_MCP_PUBLIC_URL host contains wildcard; skipping"
        );
        return;
    }
    // `url::Url::port()` returns None for default ports (443 for https, 80 for
    // http). Browsers omit the port from the Host header when it matches the
    // scheme default, so we must allowlist the bare host in that case.
    //
    // For non-standard ports, browsers include the port in Host, so we emit
    // both `host` and `host:port`.
    //
    // For standard ports we additionally emit `host:default_port` so that
    // rmcp's port-aware comparison passes even when the port is explicit.
    let explicit_port = parsed.port();
    let scheme_default_port = match parsed.scheme() {
        "https" => Some(443u16),
        "http" => Some(80u16),
        _ => None,
    };

    if let Some(p) = explicit_port {
        // Non-standard port: push `host`, `host:p` (both forms browsers use).
        push_host_variants(hosts, host, p);
        let with_port = format!("{host}:{p}");
        if !hosts.contains(&with_port) {
            hosts.push(with_port);
        }
    } else if let Some(default_port) = scheme_default_port {
        // Standard port (implicit): push bare `host` and `host:default_port`.
        // The bare host is the form browsers put in the Host header; the
        // `host:port` form satisfies rmcp's port-aware allowlist comparison.
        let bare = host.to_string();
        if !hosts.contains(&bare) {
            hosts.push(bare);
        }
        let with_default = format!("{host}:{default_port}");
        if !hosts.contains(&with_default) {
            hosts.push(with_default);
        }
    } else {
        // Unknown scheme: fall back to listen_port as before.
        push_host_variants(hosts, host, listen_port);
    }
}

fn has_port(host: &str) -> bool {
    host.rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
        .is_some()
}

pub(super) fn allowed_origins(config: &McpConfig) -> Vec<String> {
    let mut origins = vec![
        format!("http://localhost:{}", config.port),
        format!("http://127.0.0.1:{}", config.port),
    ];
    origins.extend(config.allowed_origins.iter().cloned());
    // When SYSLOG_MCP_PUBLIC_URL is set, add its origin (scheme + host + port
    // if non-default) so browser preflight from the configured public URL is
    // accepted by the CORS layer.
    if let Some(public_url) = config.auth.public_url.as_deref() {
        if let Some(origin) = extract_origin(public_url) {
            origins.push(origin);
        }
    }
    origins.sort();
    origins.dedup();
    origins
}

/// Build the Origin string from a URL (`scheme://host` or `scheme://host:port`).
///
/// Browsers omit the port from the Origin header when it matches the scheme
/// default (80 for http, 443 for https). We follow the same rule so that the
/// string we store matches what browsers actually send.
fn extract_origin(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url)
        .map_err(|e| {
            tracing::warn!(public_url = url, error = %e, "SYSLOG_MCP_PUBLIC_URL is not a valid URL; skipping origin allowlist extension");
        })
        .ok()?;
    let scheme = parsed.scheme();
    let host = parsed.host_str()?;
    // Never add wildcards.
    if host.contains('*') {
        tracing::warn!(
            host,
            "SYSLOG_MCP_PUBLIC_URL host contains wildcard; skipping origin"
        );
        return None;
    }
    let default_port = match scheme {
        "http" => Some(80u16),
        "https" => Some(443u16),
        _ => None,
    };
    let origin = match parsed.port() {
        Some(port) if default_port != Some(port) => format!("{scheme}://{host}:{port}"),
        _ => format!("{scheme}://{host}"),
    };
    Some(origin)
}

#[cfg(test)]
#[path = "rmcp_server_tests.rs"]
mod tests;
