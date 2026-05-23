use super::*;

#[test]
fn env_write_replaces_file_atomically_visible_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");
    std::fs::write(&path, "# keep\nSYSLOG_HOST=old\n").unwrap();

    write_env_value(&path, "SYSLOG_HOST", "0.0.0.0").unwrap();

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# keep\nSYSLOG_HOST=0.0.0.0\n"
    );
    assert_eq!(
        list_env_entries(&path).unwrap(),
        vec![("SYSLOG_HOST".to_string(), "0.0.0.0".to_string())]
    );
    assert!(!dir.path().read_dir().unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains(".tmp.")));
}

#[test]
fn env_remove_preserves_comments_and_other_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");
    std::fs::write(&path, "A=1\n# note\nB=2\n").unwrap();

    assert_eq!(remove_env_value(&path, "A").unwrap(), Some("1".to_string()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# note\nB=2\n");
}
