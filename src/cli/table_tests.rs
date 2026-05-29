use super::aurora_table;

#[test]
fn aurora_table_builds_without_panic() {
    let mut t = aurora_table(&["HOST", "COUNT", "LAST SEEN"]);
    t.add_row(vec![
        "myhost".to_string(),
        "42".to_string(),
        "1m ago".to_string(),
    ]);
    // Just ensure Display works
    let output = t.to_string();
    assert!(output.contains("myhost"));
    assert!(output.contains("42"));
}
