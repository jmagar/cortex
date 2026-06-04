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
    let args =
        remote_docker_events_ssh_args(Some(std::path::Path::new("/tmp/ssh_config")), "squirts");

    assert_eq!(args[0], "-o");
    assert_eq!(args[1], "IgnoreUnknown=WarnWeakCrypto");
    assert_eq!(args[2], "-F");
    assert!(args.contains(&"/tmp/ssh_config".to_string()));
    assert!(args.contains(&"BatchMode=yes".to_string()));
    assert!(args.contains(&"--".to_string()));
    assert_eq!(args[args.len() - 2], "squirts");
    assert_eq!(
        args[args.len() - 1],
        "docker events --filter type=container --format '{{json .}}'"
    );
}
