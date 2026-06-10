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
