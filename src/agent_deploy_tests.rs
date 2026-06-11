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
