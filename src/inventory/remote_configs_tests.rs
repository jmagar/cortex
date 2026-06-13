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
fn record_parser_ignores_preamble_before_first_separator() {
    let records = parse_records("noise before separator\n\u{1e}/tmp/a.conf\nbody\n");

    assert_eq!(
        records,
        vec![("/tmp/a.conf".to_string(), "body".to_string())]
    );
}

#[test]
fn record_parser_skips_records_without_body_separator() {
    let records = parse_records("\u{1e}/tmp/no-body\u{1e}/tmp/ok.yml\nservices:\n  app: {}\n\n");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, "/tmp/ok.yml");
    assert_eq!(records[0].1, "services:\n  app: {}");
}

#[test]
fn merge_output_appends_all_remote_collections_and_warnings() {
    let mut out = CollectorOutput::new("raw_configs");
    let mut remote = CollectorOutput::new("raw_configs");
    remote.warn("remote_config", "ssh failed");
    remote
        .artifacts
        .push(crate::inventory::schema::ArtifactRef {
            id: "artifact-a".to_string(),
            kind: "compose".to_string(),
            collector: "raw_configs".to_string(),
            source_host: Some("host".to_string()),
            source_path: Some("/tmp/docker-compose.yml".to_string()),
            cache_path: "/tmp/cache".to_string(),
            redaction: crate::inventory::schema::RedactionStatus::NoSecretsDetected,
            byte_len: 12,
            truncated: false,
        });

    merge_output(&mut out, remote);

    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.warnings, vec!["ssh failed".to_string()]);
    assert_eq!(out.errors.len(), 1);
}

#[tokio::test]
async fn collect_warns_and_skips_when_no_explicit_hosts_are_usable() {
    let dir = tempfile::tempdir().unwrap();
    let paths = InventoryPaths::new(dir.path().join("inventory"));
    let context = SshContext::new(crate::inventory::ssh::SshOptions::default());

    let out = collect(
        None,
        &["-bad-host".to_string()],
        &context,
        &paths,
        "run-1",
        std::time::Duration::from_millis(1),
    )
    .await;

    assert!(out.artifacts.is_empty());
    assert!(out.compose_projects.is_empty());
    assert!(
        out.warnings
            .iter()
            .any(|warning| warning.contains("no explicitly configured SSH hosts"))
    );
    assert!(
        out.warnings
            .iter()
            .any(|warning| warning.contains("rejected unsafe configured SSH host"))
    );
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
