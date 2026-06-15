use super::*;

#[test]
fn env_write_replaces_file_atomically_visible_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");
    std::fs::write(&path, "# keep\nCORTEX_RECEIVER_HOST=old\n").unwrap();

    write_env_value(&path, "CORTEX_RECEIVER_HOST", "0.0.0.0").unwrap();

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# keep\nCORTEX_RECEIVER_HOST=0.0.0.0\n"
    );
    assert_eq!(
        list_env_entries(&path).unwrap(),
        vec![("CORTEX_RECEIVER_HOST".to_string(), "0.0.0.0".to_string())]
    );
    assert!(!dir.path().read_dir().unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp.")
    }));
}

#[test]
fn env_remove_preserves_comments_and_other_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");
    std::fs::write(&path, "A=1\n# note\nB=2\n").unwrap();

    assert_eq!(remove_env_value(&path, "A").unwrap(), Some("1".to_string()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# note\nB=2\n");
}

#[test]
fn target_resolution_and_env_key_validation_cover_auto_routing() {
    assert_eq!(
        resolve_target("CORTEX_PORT", ConfigTarget::Auto).unwrap(),
        ConfigTarget::Env
    );
    assert_eq!(
        resolve_target("mcp.port", ConfigTarget::Auto).unwrap(),
        ConfigTarget::Toml
    );
    assert_eq!(
        resolve_target("lowercase", ConfigTarget::Env).unwrap(),
        ConfigTarget::Env
    );
    assert!(resolve_target("lowercase", ConfigTarget::Auto).is_err());

    assert!(looks_like_env_key("CORTEX_1"));
    assert!(looks_like_env_key("_PRIVATE"));
    assert!(!looks_like_env_key("1_BAD"));
    assert!(!looks_like_env_key("bad"));
    assert!(validate_env_key("CORTEX_TOKEN").is_ok());
    assert!(validate_env_key("cortex_token").is_err());
}

#[test]
fn env_read_list_write_and_remove_cover_missing_append_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");

    assert_eq!(read_env_kv(&path, "MISSING").unwrap(), None);
    assert_eq!(
        list_env_entries(&path).unwrap(),
        Vec::<(String, String)>::new()
    );

    write_env_value(&path, "CORTEX_PORT", "3100").unwrap();
    write_env_value(&path, "CORTEX_HOST", "127.0.0.1").unwrap();
    assert_eq!(
        read_env_kv(&path, "CORTEX_PORT").unwrap().as_deref(),
        Some("3100")
    );
    assert_eq!(
        list_env_entries(&path).unwrap(),
        vec![
            ("CORTEX_PORT".to_string(), "3100".to_string()),
            ("CORTEX_HOST".to_string(), "127.0.0.1".to_string()),
        ]
    );
    assert_eq!(remove_env_value(&path, "NOPE").unwrap(), None);
    assert!(write_env_value(&path, "BAD", "line\nbreak").is_err());
}

#[test]
fn config_output_helpers_accept_json_and_text_success_branches() {
    let path = std::path::Path::new("/tmp/cortex.env");

    print_config_value("CORTEX_PORT", Some("3100"), "env", path, false).unwrap();
    print_config_value("CORTEX_PORT", Some("3100"), "env", path, true).unwrap();
    print_config_set("CORTEX_PORT", None, "3100", "env", path, false).unwrap();
    print_config_set("CORTEX_PORT", Some("3000"), "3100", "env", path, false).unwrap();
    print_config_set("CORTEX_PORT", Some("3100"), "3100", "env", path, true).unwrap();
    print_config_unset("CORTEX_PORT", Some("3100"), "env", path, false).unwrap();
    print_config_unset("CORTEX_PORT", None, "env", path, true).unwrap();
}

#[test]
fn toml_path_override_and_atomic_temp_path_are_stable_enough_for_callers() {
    let override_path = std::path::Path::new("/tmp/cortex.toml");
    assert_eq!(toml_file_path(Some(override_path)), override_path);
    assert_eq!(
        toml_file_path(None),
        std::path::PathBuf::from("config.toml")
    );

    let temp = atomic_write_path(std::path::Path::new("/tmp/.env"));
    assert_eq!(temp.parent(), Some(std::path::Path::new("/tmp")));
    assert!(
        temp.file_name()
            .unwrap()
            .to_string_lossy()
            .contains(".env.tmp.")
    );
}
