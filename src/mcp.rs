use std::sync::Arc;

use crate::app::SyslogService;
use crate::config::McpConfig;
use crate::otlp::OtlpCounters;

mod rmcp_server;
mod routes;
mod schemas;
mod tools;

pub use rmcp_server::{
    rmcp_server, streamable_http_config, streamable_http_service, SyslogRmcpServer,
};
pub use routes::router;

/// Authentication policy attached to [`AppState`].
///
/// This is intentionally an enum (not `Option<Arc<AuthState>>` and not a
/// `bool`) so that constructing an `AppState` requires an *explicit* choice
/// between "no auth wired (loopback dev)" and "auth wired". There is no
/// `Default` impl — code that builds an `AppState` must name the variant.
///
/// Locked by the OAuth epic post-spike: the `Mounted` variant carries
/// **only** `Arc<lab_auth::state::AuthState>`. AuthContext flows per-request via
/// axum extension propagation (see `docs/internal/rmcp-auth-spike.md`); no
/// session-keyed map lives on `AppState`.
#[derive(Clone)]
pub enum AuthPolicy {
    /// No authentication is wired. Only legal when the MCP listener is
    /// bound to a loopback address (validated by [`crate::config::Config::load`]).
    LoopbackDev,
    /// Authentication is wired. The shared [`lab_auth::state::AuthState`] backs both
    /// the dual-mode middleware and the OAuth router.
    Mounted(Arc<lab_auth::state::AuthState>),
}

// Manual Debug impl: `lab_auth::state::AuthState` does not implement Debug
// (it holds RSA signing keys we never want printed), but we still want
// `AuthPolicy` to be `Debug` for use in `Result::expect`/`expect_err` and
// startup tracing.
impl std::fmt::Debug for AuthPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthPolicy::LoopbackDev => f.write_str("AuthPolicy::LoopbackDev"),
            AuthPolicy::Mounted(_) => f.write_str("AuthPolicy::Mounted(<lab_auth::AuthState>)"),
        }
    }
}

/// Shared app state
#[derive(Clone)]
pub struct AppState {
    pub service: SyslogService,
    pub config: McpConfig,
    pub otlp_counters: Arc<OtlpCounters>,
    /// Authentication policy. Construction MUST name a variant — there is no
    /// implicit default. See [`AuthPolicy`].
    pub auth_policy: AuthPolicy,
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
