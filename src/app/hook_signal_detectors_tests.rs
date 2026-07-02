use super::*;

#[test]
fn failure_statuses_detected() {
    assert!(is_hook_failure_status("failed"));
    assert!(is_hook_failure_status("blocked"));
    assert!(is_hook_failure_status("error"));
    assert!(!is_hook_failure_status("success"));
    assert!(!is_hook_failure_status("unknown"));
}

#[test]
fn timeout_detection_uses_duration_threshold() {
    assert!(is_hook_timeout("success", Some(30_001)));
    assert!(is_hook_timeout("error", Some(60_000)));
    assert!(!is_hook_timeout("success", Some(1_000)));
    assert!(!is_hook_timeout("success", None));
}

#[test]
fn output_parse_error_phrases_detected() {
    assert!(detect_hook_output_parse_error(
        "Error: invalid JSON in hook output"
    ));
    assert!(detect_hook_output_parse_error(
        "SyntaxError: Unexpected token"
    ));
    assert!(!detect_hook_output_parse_error("hook ran fine"));
}

#[test]
fn user_correction_phrases_detected() {
    assert!(detect_user_correction("That's not what I asked for"));
    assert!(detect_user_correction("Why did you run that hook again?"));
    assert!(!detect_user_correction("no new errors were found"));
}

#[test]
fn too_often_threshold() {
    assert!(!detect_hook_invoked_too_often(9));
    assert!(detect_hook_invoked_too_often(10));
    assert!(detect_hook_invoked_too_often(50));
}

#[test]
fn all_signals_list_is_stable_and_complete() {
    assert_eq!(ALL_SIGNALS.len(), 6);
    assert!(ALL_SIGNALS.contains(&SIGNAL_HOOK_FAILED));
    assert!(ALL_SIGNALS.contains(&SIGNAL_HOOK_TIMED_OUT));
    assert!(ALL_SIGNALS.contains(&SIGNAL_HOOK_NOT_INVOKED));
    assert!(ALL_SIGNALS.contains(&SIGNAL_HOOK_INVOKED_TOO_OFTEN));
    assert!(ALL_SIGNALS.contains(&SIGNAL_HOOK_OUTPUT_PARSE_ERROR));
    assert!(ALL_SIGNALS.contains(&SIGNAL_USER_CORRECTION_AFTER_HOOK));
}
