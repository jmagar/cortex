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
fn shell_quote_handles_single_quotes() {
    assert_eq!(shell_quote("/tmp/it's.yml"), "'/tmp/it'\\''s.yml'");
}
