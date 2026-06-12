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

#[test]
fn record_parser_skips_records_without_body_separator() {
    let records = parse_records("\u{1e}/tmp/no-body\u{1e}/tmp/ok.yml\nservices:\n  app: {}\n\n");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, "/tmp/ok.yml");
    assert_eq!(records[0].1, "services:\n  app: {}");
}

#[test]
fn batch_command_wraps_find_command_with_record_separator_and_byte_cap() {
    let command = batch_command("printf '%s\\n' /tmp/docker-compose.yml");

    assert!(command.starts_with("printf '%s\\n' /tmp/docker-compose.yml | while"));
    assert!(command.contains("printf '\\036%s\\n' \"$f\""));
    assert!(command.contains(&format!("head -c {} -- \"$f\"", MAX_RAW_ARTIFACT_BYTES + 1)));
}

#[test]
fn compose_and_proxy_batch_commands_search_expected_operational_roots() {
    let compose = compose_batch_command();
    let proxy = proxy_batch_command();

    assert!(compose.contains("$HOME/.cortex/compose"));
    assert!(compose.contains("/mnt/user/appdata"));
    assert!(compose.contains("-name docker-compose.yml"));
    assert!(compose.contains("head -200"));
    assert!(proxy.contains("/mnt/appdata/swag/nginx/proxy-confs"));
    assert!(proxy.contains("-name '*.conf'"));
    assert!(proxy.contains("head -300"));
}
