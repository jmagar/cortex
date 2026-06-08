use super::*;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn inventory_refresh_interval_parser_accepts_zero_disable() {
    assert_eq!(parse_inventory_refresh_interval_secs("0"), Some(0));
    assert_eq!(parse_inventory_refresh_interval_secs("300"), Some(300));
    assert_eq!(parse_inventory_refresh_interval_secs(" nope "), None);
}

#[test]
fn remote_docker_events_default_to_disabled() {
    let _guard = env_lock().lock().unwrap();
    unsafe {
        std::env::remove_var("CORTEX_INVENTORY_REMOTE_DOCKER_EVENTS");
    }
    assert!(!remote_docker_events_enabled());
    unsafe {
        std::env::set_var("CORTEX_INVENTORY_REMOTE_DOCKER_EVENTS", "true");
    }
    assert!(remote_docker_events_enabled());
    unsafe {
        std::env::remove_var("CORTEX_INVENTORY_REMOTE_DOCKER_EVENTS");
    }
}

#[test]
fn watched_config_targets_include_compose_and_proxy_paths_once() {
    let mut config = crate::inventory::InventoryConfig::from_env();
    config.compose_paths = vec![
        "/opt/edge/compose.yaml".into(),
        "/opt/edge/compose.yaml".into(),
    ];
    config.proxy_paths = vec!["/opt/swag/nginx/site.conf".into()];

    let targets = watched_config_targets(&config);

    assert_eq!(targets.len(), 2);
    assert!(targets.contains(&"/opt/edge/compose.yaml".into()));
    assert!(targets.contains(&"/opt/swag/nginx/site.conf".into()));
}

#[test]
fn watch_directories_watch_parent_for_files() {
    let targets = vec![
        "/opt/edge/compose.yaml".into(),
        "/opt/swag/nginx/site.conf".into(),
    ];

    let dirs = watch_directories(&targets);

    assert_eq!(
        dirs,
        vec![
            std::path::PathBuf::from("/opt/edge"),
            std::path::PathBuf::from("/opt/swag/nginx")
        ]
    );
}

#[test]
fn should_refresh_for_relevant_config_events_only() {
    let targets = vec!["/opt/edge/compose.yaml".into()];
    let changed = notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
        notify::event::DataChange::Content,
    )))
    .add_path("/opt/edge/compose.yaml".into());
    let access = notify::Event::new(notify::EventKind::Access(notify::event::AccessKind::Open(
        notify::event::AccessMode::Read,
    )))
    .add_path("/opt/edge/compose.yaml".into());
    let unrelated = notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
        notify::event::DataChange::Content,
    )))
    .add_path("/opt/edge/notes.txt".into());

    assert!(should_refresh_for_event(&Ok(changed), &targets));
    assert!(!should_refresh_for_event(&Ok(access), &targets));
    assert!(!should_refresh_for_event(&Ok(unrelated), &targets));
}

#[test]
fn remote_docker_events_ssh_args_include_safe_options_and_remote_command() {
    let context = crate::inventory::ssh::SshContext::new(
        crate::inventory::ssh::SshOptions::for_config(Some(std::path::Path::new(
            "/tmp/ssh_config",
        )))
        .with_event_stream_defaults(),
    );
    let args = remote_docker_events_ssh_args(&context, "squirts").unwrap();

    assert_eq!(args[0], "-o");
    assert_eq!(args[1], "IgnoreUnknown=WarnWeakCrypto");
    assert_eq!(args[2], "-F");
    assert!(args.contains(&"/tmp/ssh_config".to_string()));
    assert!(args.contains(&"BatchMode=yes".to_string()));
    assert!(args.contains(&"StrictHostKeyChecking=yes".to_string()));
    assert!(args.contains(&"ServerAliveInterval=15".to_string()));
    assert!(args.contains(&"--".to_string()));
    assert_eq!(args[args.len() - 2], "squirts");
    assert_eq!(
        args[args.len() - 1],
        "docker events --filter type=container --format '{{json .}}'"
    );
}

#[test]
fn remote_docker_events_output_sample_is_bounded() {
    let mut sample = OutputSample::default();
    sample.push_line(&"x".repeat(5000));

    let rendered = sample.as_str();
    assert!(rendered.ends_with("...<truncated>"));
    assert!(rendered.len() <= 4110);
}

#[tokio::test]
async fn remote_docker_event_stream_cancels_while_waiting_for_ssh_limiter() {
    let context = crate::inventory::ssh::SshContext::new(
        crate::inventory::ssh::SshOptions::default()
            .with_max_concurrent(1)
            .unwrap(),
    );
    let _held = context.acquire_owned().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let token = tokio_util::sync::CancellationToken::new();
    let child = token.child_token();

    let task = tokio::spawn({
        let context = context.clone();
        async move { run_remote_docker_events_once("tootie", &context, tx, child).await }
    });
    token.cancel();

    let result = tokio::time::timeout(std::time::Duration::from_millis(100), task)
        .await
        .expect("event stream task should return promptly after cancellation")
        .expect("join");
    assert!(result.is_ok());
}
