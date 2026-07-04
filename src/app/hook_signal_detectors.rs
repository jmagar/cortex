//! Deterministic, phrase-boundary keyword detectors for hook-incident anchor
//! signals plus the two purely-numeric anchors (timeout/high duration and
//! invocation frequency). Pure functions/data — no DB, no LLM. Mirrors
//! `src/app/skill_signal_detectors.rs` but keyed on hook execution/config
//! evidence instead of skill-usage evidence.

pub const SIGNAL_HOOK_FAILED: &str = "hook_failed";
pub const SIGNAL_HOOK_TIMED_OUT: &str = "hook_timed_out";
pub const SIGNAL_HOOK_NOT_INVOKED: &str = "hook_not_invoked";
pub const SIGNAL_HOOK_INVOKED_TOO_OFTEN: &str = "hook_invoked_too_often";
pub const SIGNAL_HOOK_OUTPUT_PARSE_ERROR: &str = "hook_output_parse_error";
pub const SIGNAL_USER_CORRECTION_AFTER_HOOK: &str = "user_correction_after_hook";

/// All six locked anchor signal categories, in a stable order used for
/// `signals_present` sorting and CLI `--signals` validation.
pub const ALL_SIGNALS: &[&str] = &[
    SIGNAL_HOOK_FAILED,
    SIGNAL_HOOK_TIMED_OUT,
    SIGNAL_HOOK_NOT_INVOKED,
    SIGNAL_HOOK_INVOKED_TOO_OFTEN,
    SIGNAL_HOOK_OUTPUT_PARSE_ERROR,
    SIGNAL_USER_CORRECTION_AFTER_HOOK,
];

/// Runtime statuses (from `ai_hook_events.status`, sourced from
/// `crate::scanner::hook_events::HookStatus::as_str`) that count as a hook
/// failure anchor.
const FAILURE_STATUSES: &[&str] = &["failed", "blocked", "error"];

pub fn is_hook_failure_status(status: &str) -> bool {
    FAILURE_STATUSES.contains(&status)
}

/// Above this `duration_ms`, a successful-or-unknown-status hook event still
/// counts as a `hook_timed_out` anchor (slow enough to plausibly block agent
/// flow even without an explicit timeout status).
pub const HOOK_TIMEOUT_DURATION_MS: i64 = 30_000;

pub fn is_hook_timeout(_status: &str, duration_ms: Option<i64>) -> bool {
    duration_ms.is_some_and(|ms| ms >= HOOK_TIMEOUT_DURATION_MS)
}

/// Phrases in a hook's stdout/stderr preview indicating the hook's own
/// output could not be parsed/consumed by the caller (as opposed to the hook
/// process itself exiting nonzero, which is `is_hook_failure_status`).
const OUTPUT_PARSE_ERROR_PHRASES: &[&str] = &[
    "invalid json",
    "json parse error",
    "unexpected token",
    "failed to parse hook output",
    "malformed output",
    "syntaxerror",
];

pub fn detect_hook_output_parse_error(preview: &str) -> bool {
    let lower = preview.to_ascii_lowercase();
    OUTPUT_PARSE_ERROR_PHRASES.iter().any(|p| lower.contains(p))
}

/// Phrases indicating the user is correcting or pushing back on the
/// assistant immediately after hook-provided context/instructions. Reuses
/// the same conservative phrase-level style as
/// `skill_signal_detectors::USER_CORRECTION_PHRASES`.
const USER_CORRECTION_PHRASES: &[&str] = &[
    "that's not what i asked",
    "that is not what i asked",
    "you said you would",
    "but you didn't",
    "but you did not",
    "is wrong",
    "is just wrong",
    "no, that's",
    "no, that is",
    "we wasted",
    "stop, you",
    "why did you",
    "you shouldn't have",
    "you should not have",
];

pub fn detect_user_correction(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    USER_CORRECTION_PHRASES.iter().any(|p| lower.contains(p))
}

/// Minimum invocation count for the same `(hook_event, hook_name)` pair
/// within one incident window (default 10 minutes, see
/// `search_ai_hook_incidents`'s `window_minutes`) that counts as "too
/// often". This is measured at session-window granularity, not per tool
/// call: a `PostToolUse` hook fires once per tool call by design, and a
/// single productive agentic-coding turn can easily do 10+ file
/// edits/reads within a 10-minute window (this repo's own CLAUDE.md
/// documents that pattern) without anything being wrong. Eng review fix:
/// the threshold was originally 10, chosen with a single-tool-call
/// justification that didn't match what the code actually counts
/// (invocations across the WHOLE window), causing false positives on
/// ordinary busy sessions. Raised well above a realistic productive
/// session's hook-fire count so this only fires for a genuinely
/// runaway/looping hook.
pub const HOOK_TOO_FREQUENT_THRESHOLD: usize = 30;

pub fn detect_hook_invoked_too_often(invocation_count: usize) -> bool {
    invocation_count >= HOOK_TOO_FREQUENT_THRESHOLD
}

#[cfg(test)]
#[path = "hook_signal_detectors_tests.rs"]
mod tests;
