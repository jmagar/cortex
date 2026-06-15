use super::*;

#[test]
fn list_flattens_nested_inline_tables_recursively() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "a = { b = { c = 1 }, d = true }\n").unwrap();

    let mut entries = list_toml_entries(&path).unwrap();
    entries.sort();

    assert_eq!(
        entries,
        vec![
            ("a.b.c".to_string(), "1".to_string()),
            ("a.d".to_string(), "true".to_string()),
        ]
    );
}

#[test]
fn write_toml_file_replaces_visible_content_and_cleans_temp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "old = true\n").unwrap();

    write_toml_file(&path, "new = 1\n").unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new = 1\n");
    assert!(!dir.path().read_dir().unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp.")
    }));
}

#[test]
fn toml_key_and_value_parsers_cover_scalars_and_invalid_shapes() {
    assert_eq!(
        parse_toml_key("mcp.auth.mode").unwrap(),
        vec!["mcp", "auth", "mode"]
    );
    assert!(parse_toml_key("").is_err());
    assert!(parse_toml_key("mcp..mode").is_err());

    assert_eq!(format_value(&parse_user_value("true").unwrap()), "true");
    assert_eq!(format_value(&parse_user_value("42").unwrap()), "42");
    assert_eq!(format_value(&parse_user_value("3.5").unwrap()), "3.5");
    assert_eq!(
        format_value(&parse_user_value("hello world").unwrap()),
        "\"hello world\""
    );
    assert!(parse_user_value("[a, b]").is_err());
}

#[test]
fn read_write_and_remove_toml_values_preserve_nested_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    assert_eq!(read_toml_value(&path, "mcp.port").unwrap(), None);
    assert_eq!(write_toml_value(&path, "mcp.port", "3100").unwrap(), "3100");
    assert_eq!(
        write_toml_value(&path, "mcp.auth.mode", "\"oauth\"").unwrap(),
        "\"oauth\""
    );
    assert_eq!(
        read_toml_value(&path, "mcp.auth.mode").unwrap().as_deref(),
        Some("\"oauth\"")
    );

    let mut entries = list_toml_entries(&path).unwrap();
    entries.sort();
    assert_eq!(
        entries,
        vec![
            ("mcp.auth.mode".to_string(), "\"oauth\"".to_string()),
            ("mcp.port".to_string(), "3100".to_string()),
        ]
    );

    assert_eq!(
        remove_toml_value(&path, "mcp.port").unwrap().as_deref(),
        Some("3100")
    );
    assert_eq!(remove_toml_value(&path, "mcp.missing").unwrap(), None);
}

#[test]
fn inline_table_writes_and_removes_nested_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "mcp = { auth = { mode = \"bearer\" } }\n").unwrap();

    assert_eq!(
        write_toml_value(&path, "mcp.auth.mode", "\"oauth\"").unwrap(),
        "\"oauth\""
    );
    assert_eq!(
        read_toml_value(&path, "mcp.auth.mode").unwrap().as_deref(),
        Some("\"oauth\"")
    );
    assert_eq!(
        remove_toml_value(&path, "mcp.auth.mode")
            .unwrap()
            .as_deref(),
        Some("\"oauth\"")
    );
}

#[test]
fn toml_write_and_remove_reject_non_table_parents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "mcp = 1\n").unwrap();

    assert!(write_toml_value(&path, "mcp.port", "3100").is_err());
    assert!(remove_toml_value(&path, "mcp.port").is_err());
}

#[test]
fn load_toml_document_reports_parse_errors_and_atomic_path_uses_parent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "not = [valid\n").unwrap();

    assert!(load_toml_document(&path).is_err());
    let temp = atomic_write_path(&path);
    assert_eq!(temp.parent(), Some(dir.path()));
    assert!(
        temp.file_name()
            .unwrap()
            .to_string_lossy()
            .contains("config.toml.tmp.")
    );
}
