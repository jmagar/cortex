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

/// Shared app state
#[derive(Clone)]
pub struct AppState {
    pub service: SyslogService,
    pub config: McpConfig,
    pub otlp_counters: Arc<OtlpCounters>,
}
