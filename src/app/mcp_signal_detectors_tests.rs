use super::*;

#[test]
fn detects_timeout_and_rate_limit_phrases() {
    assert!(detect_timeout_or_rate_limit("request timed out after 30s"));
    assert!(detect_timeout_or_rate_limit(
        "hit a rate limit, please retry"
    ));
    assert!(!detect_timeout_or_rate_limit("everything worked fine"));
}

#[test]
fn detects_auth_or_permission_failures() {
    assert!(detect_auth_or_permission_failure(
        "permission denied for tool"
    ));
    assert!(detect_auth_or_permission_failure(
        "Unauthorized: invalid token"
    ));
    assert!(!detect_auth_or_permission_failure("all good here"));
}

#[test]
fn detects_schema_or_validation_errors() {
    assert!(detect_schema_or_validation_error(
        "schema validation failed"
    ));
    assert!(detect_schema_or_validation_error(
        "missing required field 'query'"
    ));
    assert!(!detect_schema_or_validation_error("success"));
}

#[test]
fn detects_unknown_tool_or_server() {
    assert!(detect_unknown_tool_or_server("unknown tool: mcp__foo__bar"));
    assert!(detect_unknown_tool_or_server("server not found"));
    assert!(!detect_unknown_tool_or_server("tool executed successfully"));
}

#[test]
fn detects_user_correction_after_tool_call() {
    assert!(detect_user_correction_after_tool_call(
        "no, that's the wrong tool"
    ));
    assert!(detect_user_correction_after_tool_call(
        "that's not what I asked for"
    ));
    assert!(!detect_user_correction_after_tool_call(
        "thanks, that's correct"
    ));
}

#[test]
fn repeated_call_failure_requires_threshold() {
    assert!(!detect_repeated_call_failure(0));
    assert!(!detect_repeated_call_failure(1));
    assert!(detect_repeated_call_failure(2));
    assert!(detect_repeated_call_failure(5));
}

#[test]
fn all_signals_list_is_stable_and_complete() {
    assert_eq!(ALL_SIGNALS.len(), 6);
    assert!(ALL_SIGNALS.contains(&SIGNAL_REPEATED_CALL_FAILURE));
    assert!(ALL_SIGNALS.contains(&SIGNAL_TIMEOUT_OR_RATE_LIMIT));
    assert!(ALL_SIGNALS.contains(&SIGNAL_AUTH_OR_PERMISSION_FAILURE));
    assert!(ALL_SIGNALS.contains(&SIGNAL_SCHEMA_OR_VALIDATION_ERROR));
    assert!(ALL_SIGNALS.contains(&SIGNAL_UNKNOWN_TOOL_OR_SERVER));
    assert!(ALL_SIGNALS.contains(&SIGNAL_USER_CORRECTION_AFTER_TOOL_CALL));
}
