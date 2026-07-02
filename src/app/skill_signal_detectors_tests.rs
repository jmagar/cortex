use super::*;

#[test]
fn detects_direct_correction_phrases() {
    let positives = [
        "That's not what I asked for, please redo it.",
        "You said you would run the tests but you didn't.",
        "This is just wrong, revert it.",
        "No, that's the wrong file entirely.",
        "We wasted twenty minutes on this dead end.",
        "All you had to say was 'I don't know'.",
        "Stop, you're going in circles.",
        "You didn't need to touch that config at all.",
    ];
    for msg in positives {
        assert!(
            detect_user_correction(msg),
            "expected correction hit for: {msg}"
        );
    }
}

#[test]
fn does_not_flag_unrelated_negatives() {
    let negatives = [
        "No new errors were found in the log scan.",
        "The stop hook fired successfully.",
        "You said the deploy finished — confirming that now.",
        "wrongdoing was not detected in the audit",
    ];
    for msg in negatives {
        assert!(
            !detect_user_correction(msg),
            "unexpected correction hit for: {msg}"
        );
    }
}

#[test]
fn detects_tool_failure_phrases() {
    let positives = [
        "Command exited with exit code 1",
        "bash: permission denied",
        "error: file not found",
        "operation timed out after 30s",
        "failed to connect to database",
        "database is locked",
        "429 rate limit exceeded",
    ];
    for msg in positives {
        assert!(
            detect_tool_failure(msg),
            "expected tool-failure hit for: {msg}"
        );
    }
}

#[test]
fn does_not_flag_successful_tool_output_as_failure() {
    let negatives = [
        "build completed successfully in 4.2s",
        "all 42 tests passed",
        "pushed to origin/main",
    ];
    for msg in negatives {
        assert!(
            !detect_tool_failure(msg),
            "unexpected tool-failure hit for: {msg}"
        );
    }
}

#[test]
fn detects_scope_or_source_confusion_phrases() {
    let positives = [
        "wait, this is the wrong repo entirely",
        "that data is stale, we're looking at memory not the live system",
        "you're using the wrong source of truth here",
        "this is memory-vs-live confusion, check the running container",
    ];
    for msg in positives {
        assert!(
            detect_scope_or_source_confusion(msg),
            "expected scope/source confusion hit for: {msg}"
        );
    }
}

#[test]
fn does_not_flag_unrelated_text_as_scope_confusion() {
    let negatives = [
        "the repo was cloned successfully",
        "source code review complete",
        "memory usage is within limits",
    ];
    for msg in negatives {
        assert!(
            !detect_scope_or_source_confusion(msg),
            "unexpected scope/source confusion hit for: {msg}"
        );
    }
}

#[test]
fn detects_ignored_instruction_phrases() {
    let positives = [
        "you claimed success without any verification",
        "you should have created a bead for this but didn't",
        "you used the wrong transport for this call",
        "you searched the raw web instead of using axon",
        "you stopped at the plan instead of implementing it",
    ];
    for msg in positives {
        assert!(
            detect_ignored_instruction(msg),
            "expected ignored-instruction hit for: {msg}"
        );
    }
}

#[test]
fn does_not_flag_compliant_text_as_ignored_instruction() {
    let negatives = [
        "verification passed, all tests green",
        "created bead cortex-142 for the follow-up",
        "used axon to research this before answering",
    ];
    for msg in negatives {
        assert!(
            !detect_ignored_instruction(msg),
            "unexpected ignored-instruction hit for: {msg}"
        );
    }
}

#[test]
fn overlong_loop_requires_both_volume_and_negative_signal() {
    // Long-but-successful: many tool calls, no correction/frustration — must NOT trigger.
    assert!(!detect_overlong_loop(3, 40, false));
    // Short loop with a correction — not "overlong", must NOT trigger.
    assert!(!detect_overlong_loop(1, 3, true));
    // Long loop WITH a correction/frustration signal — must trigger.
    assert!(detect_overlong_loop(2, 25, true));
}
