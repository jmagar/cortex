//! Single authoritative table of all MCP actions.
//!
//! Previously the action list was spread across three arrays in two files:
//! `SYSLOG_ACTIONS` in `schemas.rs` (for the JSON schema `enum`),
//! `READ_ONLY_ACTIONS` and `ADMIN_ACTIONS` in `rmcp_server.rs` (for scope
//! gating). Adding a new action required editing all three in lockstep; drift
//! was a constant source of bugs.
//!
//! Now there is one table: [`ACTION_SPECS`]. Every derived property
//! (`action_names`, `required_scope_for`) is computed from it. Only this file
//! needs to change when a new action is added.

/// The scope required to invoke a given action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    /// Read-only query. Requires `syslog:read`.
    Read,
    /// State-mutating or outbound-notification operation. Requires
    /// `syslog:admin`.
    Admin,
    /// Informational action — auth context required when policy is `Mounted`,
    /// but no scope gate. Currently only `help`.
    InfoOnly,
}

/// Metadata for a single MCP action.
#[derive(Debug)]
pub(super) struct ActionSpec {
    /// Action name as passed in the `action` field of the MCP request.
    pub name: &'static str,
    /// Required scope (or `InfoOnly` for auth-but-no-scope actions).
    pub scope: Scope,
    /// Short human-readable description (reserved for future help text
    /// generation; not yet consumed outside this module).
    #[allow(dead_code)]
    pub description: &'static str,
}

/// The single authoritative table of all supported MCP actions.
///
/// # Maintenance
/// When adding a new action:
/// 1. Add an `ActionSpec` row here.
/// 2. Add a handler branch in `src/mcp/tools.rs`.
///
/// No other file needs to change for basic action registration.
pub(super) const ACTION_SPECS: &[ActionSpec] = &[
    // ── Read-only queries ──────────────────────────────────────────────────
    ActionSpec {
        name: "search",
        scope: Scope::Read,
        description: "Full-text search over syslog messages",
    },
    ActionSpec {
        name: "tail",
        scope: Scope::Read,
        description: "Stream the most recent log entries",
    },
    ActionSpec {
        name: "errors",
        scope: Scope::Read,
        description: "List recent error-level log entries",
    },
    ActionSpec {
        name: "hosts",
        scope: Scope::Read,
        description: "Enumerate all known source hostnames",
    },
    ActionSpec {
        name: "correlate",
        scope: Scope::Read,
        description: "Correlate events across hosts/services",
    },
    ActionSpec {
        name: "stats",
        scope: Scope::Read,
        description: "Aggregate log statistics",
    },
    ActionSpec {
        name: "status",
        scope: Scope::Read,
        description: "Server health and ingestion status",
    },
    ActionSpec {
        name: "apps",
        scope: Scope::Read,
        description: "Enumerate all known application names",
    },
    ActionSpec {
        name: "sessions",
        scope: Scope::Read,
        description: "List AI transcript sessions",
    },
    ActionSpec {
        name: "search_sessions",
        scope: Scope::Read,
        description: "Full-text search over AI transcript sessions",
    },
    ActionSpec {
        name: "abuse",
        scope: Scope::Read,
        description: "Detect resource-abuse patterns in AI sessions",
    },
    ActionSpec {
        name: "abuse_incidents",
        scope: Scope::Read,
        description: "List detected abuse incidents",
    },
    ActionSpec {
        name: "abuse_investigate",
        scope: Scope::Read,
        description: "Deep-dive investigation of an abuse incident",
    },
    ActionSpec {
        name: "ai_correlate",
        scope: Scope::Read,
        description: "Correlate AI transcript events with syslog",
    },
    ActionSpec {
        name: "usage_blocks",
        scope: Scope::Read,
        description: "Summarise AI session usage by project",
    },
    ActionSpec {
        name: "project_context",
        scope: Scope::Read,
        description: "Full project context from AI transcripts",
    },
    ActionSpec {
        name: "list_ai_tools",
        scope: Scope::Read,
        description: "List AI tools observed in transcripts",
    },
    ActionSpec {
        name: "list_ai_projects",
        scope: Scope::Read,
        description: "List AI projects with transcript activity",
    },
    ActionSpec {
        name: "source_ips",
        scope: Scope::Read,
        description: "Enumerate unique source IP addresses",
    },
    ActionSpec {
        name: "timeline",
        scope: Scope::Read,
        description: "Log volume over time (bucketed)",
    },
    ActionSpec {
        name: "patterns",
        scope: Scope::Read,
        description: "Recurring message patterns",
    },
    ActionSpec {
        name: "context",
        scope: Scope::Read,
        description: "Contextual log entries around a pivot",
    },
    ActionSpec {
        name: "get",
        scope: Scope::Read,
        description: "Fetch a single log entry by ID",
    },
    ActionSpec {
        name: "ingest_rate",
        scope: Scope::Read,
        description: "Current log ingestion rate",
    },
    ActionSpec {
        name: "silent_hosts",
        scope: Scope::Read,
        description: "Hosts that have gone silent",
    },
    ActionSpec {
        name: "clock_skew",
        scope: Scope::Read,
        description: "Detect clock skew between hosts",
    },
    ActionSpec {
        name: "anomalies",
        scope: Scope::Read,
        description: "Detect log-volume anomalies",
    },
    ActionSpec {
        name: "compare",
        scope: Scope::Read,
        description: "Compare log patterns between time windows",
    },
    ActionSpec {
        name: "compose_status",
        scope: Scope::Read,
        description: "Docker Compose stack status",
    },
    ActionSpec {
        name: "compose_doctor",
        scope: Scope::Read,
        description: "Docker Compose coordination diagnostics",
    },
    ActionSpec {
        name: "unaddressed_errors",
        scope: Scope::Read,
        description: "List unacknowledged error signatures",
    },
    ActionSpec {
        name: "notifications_recent",
        scope: Scope::Read,
        description: "Recent notification firings",
    },
    ActionSpec {
        name: "similar_incidents",
        scope: Scope::Read,
        description: "Find similar past incidents",
    },
    ActionSpec {
        name: "ask_history",
        scope: Scope::Read,
        description: "Query AI transcript history",
    },
    ActionSpec {
        name: "incident_context",
        scope: Scope::Read,
        description: "Full context for an incident",
    },
    // ── Admin / write actions ──────────────────────────────────────────────
    ActionSpec {
        name: "ack_error",
        scope: Scope::Admin,
        description: "Acknowledge an error signature",
    },
    ActionSpec {
        name: "unack_error",
        scope: Scope::Admin,
        description: "Revoke an error signature acknowledgement",
    },
    ActionSpec {
        name: "notifications_test",
        scope: Scope::Admin,
        description: "Send a test notification via Apprise",
    },
    // ── Informational (auth required, no scope gate) ───────────────────────
    ActionSpec {
        name: "help",
        scope: Scope::InfoOnly,
        description: "List available actions and their parameters",
    },
];

/// All action names in registration order. Used to populate the JSON schema
/// `enum` in `tool_definitions()` — derives from `ACTION_SPECS` so the schema
/// and scope table cannot drift.
pub(super) fn action_names() -> Vec<&'static str> {
    ACTION_SPECS.iter().map(|s| s.name).collect()
}

/// Map an action name to its required MCP scope string.
///
/// - `None` for `InfoOnly` actions (auth context required when Mounted, but no
///   scope gate).
/// - `Some("syslog:read")` / `Some("syslog:admin")` for normal actions.
/// - `Some("syslog:__deny__")` for unknown actions — a sentinel scope that is
///   never granted, so unknown actions are denied at the auth layer rather than
///   falling through to the dispatcher. Fail-closed.
pub(super) fn required_scope_for(action: &str) -> Option<&'static str> {
    match ACTION_SPECS.iter().find(|s| s.name == action) {
        Some(spec) => match spec.scope {
            Scope::InfoOnly => None,
            Scope::Read => Some("syslog:read"),
            Scope::Admin => Some("syslog:admin"),
        },
        None => Some("syslog:__deny__"),
    }
}
