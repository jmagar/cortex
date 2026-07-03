//! Deterministic, phrase-boundary keyword detectors for MCP-incident anchor
//! signals. Pure functions over log message text and `ai_mcp_events` row
//! fields — no DB, no LLM. Mirrors `src/app/skill_signal_detectors.rs`'s
//! phrase-boundary matching style, retargeted at the MCP incident anchor
//! list from GH #94's "MCP assessment design" section: repeated call
//! failures, `is_error`, timeout/rate-limit, schema errors, unknown
//! tool/server, and user correction after tool misuse.

pub const SIGNAL_REPEATED_CALL_FAILURE: &str = "repeated_call_failure";
pub const SIGNAL_TIMEOUT_OR_RATE_LIMIT: &str = "timeout_or_rate_limit";
pub const SIGNAL_AUTH_OR_PERMISSION_FAILURE: &str = "auth_or_permission_failure";
pub const SIGNAL_SCHEMA_OR_VALIDATION_ERROR: &str = "schema_or_validation_error";
pub const SIGNAL_UNKNOWN_TOOL_OR_SERVER: &str = "unknown_tool_or_server";
pub const SIGNAL_USER_CORRECTION_AFTER_TOOL_CALL: &str = "user_correction_after_tool_call";

/// All six locked anchor signal categories, in a stable order used for
/// `signals_present` sorting and CLI `--signals` validation.
pub const ALL_SIGNALS: &[&str] = &[
    SIGNAL_REPEATED_CALL_FAILURE,
    SIGNAL_TIMEOUT_OR_RATE_LIMIT,
    SIGNAL_AUTH_OR_PERMISSION_FAILURE,
    SIGNAL_SCHEMA_OR_VALIDATION_ERROR,
    SIGNAL_UNKNOWN_TOOL_OR_SERVER,
    SIGNAL_USER_CORRECTION_AFTER_TOOL_CALL,
];

const TIMEOUT_OR_RATE_LIMIT_PHRASES: &[&str] = &[
    "timed out",
    "timeout",
    "rate limit",
    "rate-limited",
    "too many requests",
];

const AUTH_OR_PERMISSION_PHRASES: &[&str] = &[
    "permission denied",
    "unauthorized",
    "forbidden",
    "authentication failed",
    "auth failed",
    "invalid token",
    "invalid credentials",
];

const SCHEMA_OR_VALIDATION_PHRASES: &[&str] = &[
    "schema validation",
    "invalid parameters",
    "invalid arguments",
    "missing required",
    "does not match schema",
    "validation error",
    "invalidparams",
];

const UNKNOWN_TOOL_OR_SERVER_PHRASES: &[&str] = &[
    "unknown tool",
    "tool not found",
    "server unavailable",
    "server not found",
    "no such tool",
    "not connected",
    "disconnected",
    "mcp server error",
];

/// Reused from the skill-incident correction phrase list style (same
/// conservative multi-word-phrase design) but scoped to tool-call
/// misuse/misinterpretation follow-up.
const USER_CORRECTION_PHRASES: &[&str] = &[
    "that's not what i asked",
    "that is not what i asked",
    "wrong tool",
    "not the right tool",
    "you used the wrong",
    "that's the wrong",
    "that is the wrong",
    "no, that's wrong",
    "no, that is wrong",
];

fn contains_any_phrase(haystack_lower: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|p| haystack_lower.contains(p))
}

pub fn detect_timeout_or_rate_limit(message: &str) -> bool {
    contains_any_phrase(&message.to_ascii_lowercase(), TIMEOUT_OR_RATE_LIMIT_PHRASES)
}

pub fn detect_auth_or_permission_failure(message: &str) -> bool {
    contains_any_phrase(&message.to_ascii_lowercase(), AUTH_OR_PERMISSION_PHRASES)
}

pub fn detect_schema_or_validation_error(message: &str) -> bool {
    contains_any_phrase(&message.to_ascii_lowercase(), SCHEMA_OR_VALIDATION_PHRASES)
}

pub fn detect_unknown_tool_or_server(message: &str) -> bool {
    contains_any_phrase(
        &message.to_ascii_lowercase(),
        UNKNOWN_TOOL_OR_SERVER_PHRASES,
    )
}

pub fn detect_user_correction_after_tool_call(message: &str) -> bool {
    contains_any_phrase(&message.to_ascii_lowercase(), USER_CORRECTION_PHRASES)
}

/// Minimum error-event count for the same `(mcp_server, mcp_tool)` pair
/// within a group that counts as a "repeated call failure" anchor, as
/// opposed to a single one-off error.
pub const REPEATED_FAILURE_THRESHOLD: usize = 2;

pub fn detect_repeated_call_failure(error_event_count: usize) -> bool {
    error_event_count >= REPEATED_FAILURE_THRESHOLD
}

#[cfg(test)]
#[path = "mcp_signal_detectors_tests.rs"]
mod tests;
