use super::*;

#[test]
fn cortex_home_dir_can_be_inferred_from_user_local_bin_binary() {
    let exe = std::path::Path::new("/home/jmagar/.local/bin/cortex");

    assert_eq!(
        cortex_home_dir_from_exe_path(exe).as_deref(),
        Some(std::path::Path::new("/home/jmagar/.cortex"))
    );
}

#[test]
fn cortex_home_dir_can_be_inferred_from_user_workspace_binary() {
    let exe = std::path::Path::new("/home/jmagar/workspace/cortex/target/release/cortex");

    assert_eq!(
        cortex_home_dir_from_exe_path(exe).as_deref(),
        Some(std::path::Path::new("/home/jmagar/.cortex"))
    );
}

#[test]
fn cortex_home_dir_is_not_inferred_from_non_home_binary() {
    let exe = std::path::Path::new("/usr/local/bin/cortex");

    assert_eq!(cortex_home_dir_from_exe_path(exe), None);
}

/// full-review QM6: only a filesystem-root `/home` (or `/var/home`)
/// qualifies — nested directories merely NAMED `home` must not redirect
/// config resolution.
#[test]
fn cortex_home_dir_is_not_inferred_from_nested_home_directories() {
    for exe in [
        "/opt/home/svc/bin/cortex",
        "/tmp/home/evil/cortex",
        "/build/home/ci/cortex",
    ] {
        assert_eq!(
            cortex_home_dir_from_exe_path(std::path::Path::new(exe)),
            None,
            "nested home dir must not match: {exe}"
        );
    }
}

#[test]
fn cortex_home_dir_is_inferred_from_ostree_var_home() {
    let exe = std::path::Path::new("/var/home/jmagar/.local/bin/cortex");

    assert_eq!(
        cortex_home_dir_from_exe_path(exe).as_deref(),
        Some(std::path::Path::new("/var/home/jmagar/.cortex"))
    );
}

#[test]
fn db_path_from_setup_env_uses_absolute_compose_data_volume() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    std::fs::write(
        &env_path,
        "CORTEX_DB_PATH=/data/cortex.db\nCORTEX_DATA_VOLUME=/srv/syslog-data\n",
    )
    .unwrap();

    assert_eq!(
        db_path_from_setup_env(&env_path).unwrap(),
        Some(std::path::PathBuf::from("/srv/syslog-data/cortex.db"))
    );
}

#[test]
fn db_path_from_setup_env_rejects_container_db_without_absolute_data_volume() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join(".env");
    std::fs::write(
        &env_path,
        "CORTEX_DB_PATH=/data/cortex.db\nCORTEX_DATA_VOLUME=syslog-data\n",
    )
    .unwrap();

    let err = db_path_from_setup_env(&env_path).unwrap_err();
    assert!(
        err.to_string()
            .contains("CORTEX_DATA_VOLUME is not absolute")
    );
}

#[test]
fn validate_executable_path_rejects_debug_build_paths_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join(".cache/cargo/debug/cortex");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, "#!/bin/sh\n").unwrap();

    let err = validate_executable_path(bin).unwrap_err();
    assert!(err.to_string().contains("debug/worktree binary"));
}

#[test]
fn validate_db_path_rejects_relative_without_creating_parent() {
    let relative_dir = format!(
        "relative-db-dir-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let relative = std::path::PathBuf::from(&relative_dir).join("cortex.db");
    let err = validate_db_path(relative).unwrap_err();
    assert!(err.to_string().contains("must be absolute"));
    assert!(!std::path::Path::new(&relative_dir).exists());
}

#[test]
fn validate_db_path_rejects_root_parent_and_unit_breaking_chars() {
    let root_db = std::path::PathBuf::from("/cortex.db");
    let err = validate_db_path(root_db).unwrap_err();
    assert!(err.to_string().contains("non-root directory"));

    let spaced = std::path::PathBuf::from("/tmp/cortex mcp/cortex.db");
    let err = validate_db_path(spaced).unwrap_err();
    assert!(err.to_string().contains("unsupported character"));
}

#[cfg(unix)]
#[test]
fn private_and_executable_writers_reject_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let private_link = dir.path().join("private-link");
    let exec_link = dir.path().join("exec-link");
    std::fs::write(&target, "keep").unwrap();
    std::os::unix::fs::symlink(&target, &private_link).unwrap();
    std::os::unix::fs::symlink(&target, &exec_link).unwrap();

    assert!(write_private_file(&private_link, "secret").is_err());
    assert!(write_executable_file(&exec_link, "#!/bin/sh\n").is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
}
