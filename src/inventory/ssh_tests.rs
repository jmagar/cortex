use super::*;

#[test]
fn ssh_host_parser_keeps_concrete_hosts_only() {
    let hosts = parse_ssh_hosts(
        r#"
Host *
  IdentityFile ~/.ssh/id_ed25519
Host tootie shart
  User root
Host github.com
  HostName ssh.github.com
Host steamy-*
  User jmagar
Host dookie
  User jmagar
"#,
    );

    assert_eq!(hosts, vec!["tootie", "shart", "dookie"]);
}

#[test]
fn ssh_args_ignore_newer_config_options_before_loading_config() {
    let args = ssh_args(
        Some(std::path::Path::new("/tmp/ssh_config")),
        "tootie",
        "true",
    );

    assert_eq!(args[0], "-o");
    assert_eq!(args[1], "IgnoreUnknown=WarnWeakCrypto");
    assert_eq!(args[2], "-F");
    assert_eq!(args[3], "/tmp/ssh_config");
}
