use super::*;
use std::process::ExitStatus;

#[cfg(unix)]
fn output(status: i32, stdout: &[u8], stderr: &[u8]) -> std::process::Output {
    use std::os::unix::process::ExitStatusExt;

    std::process::Output {
        status: ExitStatus::from_raw(status << 8),
        stdout: stdout.to_vec(),
        stderr: stderr.to_vec(),
    }
}

#[cfg(unix)]
#[test]
fn systemctl_error_message_prefers_stdout_state() {
    let output = output(1, b"inactive\n", b"ignored stderr\n");

    assert_eq!(systemctl_error_message(&output), "inactive");
}

#[cfg(unix)]
#[test]
fn systemctl_error_message_falls_back_to_first_stderr_line() {
    let output = output(1, b"\n", b"Unit missing\nsecond line\n");

    assert_eq!(systemctl_error_message(&output), "Unit missing");
}

#[cfg(unix)]
#[test]
fn systemctl_error_message_has_default_when_streams_are_empty() {
    let output = output(1, b"", b"");

    assert_eq!(systemctl_error_message(&output), "systemctl --user failed");
}

#[test]
fn inferred_user_bus_env_matches_current_uid_runtime_bus_when_present() {
    let result = inferred_user_bus_env();
    let uid = super::super::current_uid_gid().0;

    if let Some((runtime_dir, bus_address)) = result {
        assert_eq!(runtime_dir, PathBuf::from(format!("/run/user/{uid}")));
        assert_eq!(bus_address, format!("unix:path=/run/user/{uid}/bus"));
    }
}
