use super::*;

#[test]
fn parse_ssh_config_skips_wildcards_and_github() {
    let config = "Host *\n  ServerAliveInterval 60\n\nHost dookie\n  HostName 100.88.16.79\n\nHost github.com\n  User git\n\nHost tootie squirts\n  User jmagar\n";
    let hosts = parse_ssh_config_hosts(config);
    assert_eq!(hosts, vec!["dookie", "tootie", "squirts"]);
}

#[test]
fn parse_ssh_config_deduplicates() {
    let config = "Host dookie\nHost dookie\nHost tootie\n";
    let hosts = parse_ssh_config_hosts(config);
    assert_eq!(hosts, vec!["dookie", "tootie"]);
}

#[test]
fn parse_ssh_config_skips_unsafe_hosts() {
    let config = "Host dookie\nHost -bad\nHost ok-host\n";
    let hosts = parse_ssh_config_hosts(config);
    assert_eq!(hosts, vec!["dookie", "ok-host"]);
}

#[test]
fn shell_quote_escapes_single_quotes() {
    assert_eq!(shell_quote("hello"), "'hello'");
    assert_eq!(shell_quote("it's"), "'it'\"'\"'s'");
}

#[test]
fn host_probe_label_formats_reachable_with_agent() {
    let probe = HostProbe {
        host: "dookie".to_string(),
        reachable: true,
        cortex_version: Some("1.17.0".to_string()),
        agent_active: Some(true),
    };
    let label = probe.display_label();
    assert!(label.contains("✓"));
    assert!(label.contains("1.17.0"));
    assert!(label.contains("agent:active"));
}

#[test]
fn host_probe_label_formats_unreachable() {
    let probe = HostProbe {
        host: "steamy".to_string(),
        reachable: false,
        cortex_version: None,
        agent_active: None,
    };
    let label = probe.display_label();
    assert!(label.contains("✗"));
}

#[test]
fn parse_host_selection_accepts_all_and_dedupes_numbers() {
    assert_eq!(parse_host_selection("all", 3).unwrap(), vec![0, 1, 2]);
    assert_eq!(parse_host_selection("2, 1 2", 3).unwrap(), vec![0, 1]);
}

#[test]
fn parse_host_selection_rejects_empty_invalid_and_out_of_range() {
    assert!(parse_host_selection("", 3).is_err());
    assert!(parse_host_selection("wat", 3).is_err());
    assert!(parse_host_selection("0", 3).is_err());
    assert!(parse_host_selection("4", 3).is_err());
}

#[test]
fn unraid_constants_wire_socket_and_host_syslog() {
    assert_eq!(UNRAID_CONTAINER_SYSLOG, "/host/var/log/syslog");
    assert_eq!(
        crate::heartbeat_agent::DEFAULT_DOCKER_URL,
        "unix:///var/run/docker.sock"
    );
}

#[test]
fn deploy_syslog_target_derives_from_heartbeat_url() {
    assert_eq!(
        deploy_syslog_target(Some("https://cortex.example.test:3100")),
        Some("cortex.example.test:1514".to_string())
    );
}
