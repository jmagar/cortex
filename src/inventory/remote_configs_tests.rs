use super::*;

#[test]
fn record_parser_splits_path_and_body() {
    let records =
        parse_records("\u{1e}/tmp/a.conf\nserver_name a.test;\n\u{1e}/tmp/b.yml\nservices:\n");

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].0, "/tmp/a.conf");
    assert_eq!(records[0].1, "server_name a.test;");
    assert_eq!(records[1].0, "/tmp/b.yml");
    assert_eq!(records[1].1, "services:");
}
