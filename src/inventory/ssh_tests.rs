use super::*;
use crate::inventory::process::CommandOutput;
use futures_util::future::join_all;
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

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
    let args = SshOptions {
        config: Some(std::path::PathBuf::from("/tmp/ssh_config")),
        ..SshOptions::default()
    }
    .ssh_args("tootie", "true")
    .unwrap();

    assert_eq!(args[0], "-o");
    assert_eq!(args[1], "IgnoreUnknown=WarnWeakCrypto");
    assert_eq!(args[2], "-F");
    assert_eq!(args[3], "/tmp/ssh_config");
}

#[test]
fn ssh_args_reject_option_like_hosts_and_use_strict_host_keys_by_default() {
    assert!(SshOptions::default()
        .ssh_args("-oProxyCommand=touch /tmp/pwned", "true")
        .unwrap_err()
        .to_string()
        .contains("unsafe ssh host"));

    let args = SshOptions {
        known_hosts: Some(std::path::PathBuf::from("/tmp/cortex_known_hosts")),
        ..SshOptions::default()
    }
    .ssh_args("tootie", "true")
    .unwrap();

    assert!(args.contains(&"StrictHostKeyChecking=yes".to_string()));
    assert!(args.contains(&"UserKnownHostsFile=/tmp/cortex_known_hosts".to_string()));
    assert!(args.contains(&"--".to_string()));
    assert_eq!(args[args.len() - 2], "tootie");
}

#[test]
fn ssh_args_can_opt_into_trust_on_first_use() {
    let args = SshOptions {
        host_key_policy: SshHostKeyPolicy::AcceptNew,
        ..SshOptions::default()
    }
    .ssh_args("bootstrap-host", "true")
    .unwrap();

    assert!(args.contains(&"StrictHostKeyChecking=accept-new".to_string()));
}

#[tokio::test]
async fn ssh_context_limits_concurrency_and_retries_with_backoff() {
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::new(Mutex::new(BTreeMap::<String, usize>::new()));
    let active_for_runner = Arc::clone(&active);
    let max_active_for_runner = Arc::clone(&max_active);
    let attempts_for_runner = Arc::clone(&attempts);

    let context = SshContext::with_runner_for_test(
        SshOptions {
            max_concurrent: 2,
            retry_attempts: 2,
            retry_initial_backoff: Duration::from_millis(1),
            ..SshOptions::default()
        },
        move |args, _timeout| {
            let active = Arc::clone(&active_for_runner);
            let max_active = Arc::clone(&max_active_for_runner);
            let attempts = Arc::clone(&attempts_for_runner);
            Box::pin(async move {
                let host = args[args.len() - 2].clone();
                let now_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_active.fetch_max(now_active, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                active.fetch_sub(1, Ordering::SeqCst);

                let mut attempts = attempts.lock().unwrap();
                let attempt = attempts.entry(host).or_default();
                *attempt += 1;
                if *attempt == 1 {
                    anyhow::bail!("transient ssh failure");
                }
                Ok(CommandOutput {
                    status: Some(0),
                    stdout: "ok".to_string(),
                    stderr: String::new(),
                    elapsed_ms: 1,
                    truncated: false,
                })
            })
        },
    );

    let futures = ["a", "b", "c", "d"]
        .into_iter()
        .map(|host| context.run(host, "true", Duration::from_secs(1)));
    let results = join_all(futures).await;

    assert!(results.iter().all(Result::is_ok));
    assert_eq!(max_active.load(Ordering::SeqCst), 2);
    assert_eq!(
        attempts
            .lock()
            .unwrap()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        vec![2, 2, 2, 2]
    );
}

#[test]
fn legacy_ssh_args_wrapper_still_uses_safe_builder() {
    let args = ssh_args(
        Some(std::path::Path::new("/tmp/ssh_config")),
        "tootie",
        "true",
    )
    .unwrap();

    assert_eq!(args[0], "-o");
    assert_eq!(args[1], "IgnoreUnknown=WarnWeakCrypto");
    assert_eq!(args[2], "-F");
    assert_eq!(args[3], "/tmp/ssh_config");
}
