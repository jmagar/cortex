use super::*;

#[cfg(unix)]
#[test]
fn install_agent_command_files_hardens_existing_spool_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join("state");
    std::fs::create_dir(&state_dir).unwrap();
    let spool = state_dir.join("agent-command.jsonl");
    std::fs::write(&spool, "keep\n").unwrap();
    std::fs::set_permissions(&spool, std::fs::Permissions::from_mode(0o644)).unwrap();
    let wrapper = dir.path().join("bin/wrapper");

    let phase = install_agent_command_files(&wrapper, &spool, &state_dir, Path::new("/bin/sh"))
        .expect("install files");

    assert_eq!(phase.status, SetupStatus::Ok);
    assert_eq!(std::fs::read_to_string(&spool).unwrap(), "keep\n");
    let mode = std::fs::metadata(&spool).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[test]
fn agent_command_state_phase_rejects_spool_symlink() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join("state");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let target = dir.path().join("target.jsonl");
    std::fs::write(&target, "").unwrap();
    let spool = state_dir.join("agent-command.jsonl");
    std::os::unix::fs::symlink(&target, &spool).unwrap();

    let phase = agent_command_state_phase(&state_dir, &spool);

    assert_eq!(phase.status, SetupStatus::Error);
    assert!(phase.detail.contains("regular file"));
}

#[test]
fn agent_command_state_phase_rejects_spool_directory() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join("state");
    std::fs::create_dir(&state_dir).unwrap();
    let spool = state_dir.join("agent-command.jsonl");
    std::fs::create_dir(&spool).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&spool, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let phase = agent_command_state_phase(&state_dir, &spool);

    assert_eq!(phase.status, SetupStatus::Error);
    assert!(phase.detail.contains("regular file"));
}

#[test]
fn agent_command_env_phase_accepts_claude_settings_json() {
    let home = tempfile::tempdir().unwrap();
    let claude_dir = home.path().join(".claude");
    std::fs::create_dir(&claude_dir).unwrap();
    let wrapper = home.path().join(".local/bin/cortex-agent-command-wrapper");
    std::fs::write(
        claude_dir.join("settings.json"),
        format!(
            r#"{{"env":{{"CLAUDE_CODE_SHELL_PREFIX":"{}"}}}}"#,
            wrapper.display()
        ),
    )
    .unwrap();

    let phase = agent_command_env_phase(&wrapper, home.path());

    assert_eq!(phase.status, SetupStatus::Ok);
}
