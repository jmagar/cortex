use std::process::Command;

#[test]
fn help_lists_sessions_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_syslog"))
        .arg("--help")
        .output()
        .unwrap();

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("syslog sessions"),
        "help output should list the sessions command, got:\n{stderr}"
    );
}
