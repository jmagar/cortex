use super::*;

#[test]
fn ensure_command_success_accepts_zero_exit_without_timeout() {
    let output = syslog_mcp::compose::CommandOutput {
        exit_status: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        timed_out: false,
        timeout_cleanup: None,
    };

    ensure_command_success(&output).unwrap();
}

#[test]
fn ensure_command_success_reports_failed_status_and_stderr() {
    let output = syslog_mcp::compose::CommandOutput {
        exit_status: Some(1),
        stdout: String::new(),
        stderr: "bad compose".to_string(),
        stdout_truncated: false,
        stderr_truncated: false,
        timed_out: false,
        timeout_cleanup: None,
    };

    let err = ensure_command_success(&output).unwrap_err().to_string();
    assert!(err.contains("status=Some(1)"));
    assert!(err.contains("bad compose"));
}
