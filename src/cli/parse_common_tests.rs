use super::*;

#[test]
fn flag_cursor_accepts_negative_signed_values() {
    let args = vec!["-1".to_string()];
    let mut cursor = FlagCursor::new(&args);

    assert_eq!(cursor.value("--since").unwrap(), "-1");
}

#[test]
fn flag_cursor_still_rejects_next_flag_as_missing_value() {
    let args = vec!["--json".to_string()];
    let mut cursor = FlagCursor::new(&args);

    assert!(cursor
        .value("--since")
        .unwrap_err()
        .to_string()
        .contains("requires a value"));
}
