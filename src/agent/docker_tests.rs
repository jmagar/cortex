use super::*;

#[test]
fn container_display_name_prefers_first_docker_name_without_leading_slash() {
    assert_eq!(
        container_display_name(
            "abcdef1234567890",
            Some(vec!["/cortex".to_string(), "/alias".to_string()])
        ),
        "cortex"
    );
}

#[test]
fn container_display_name_falls_back_to_short_id_when_names_missing() {
    assert_eq!(
        container_display_name("abcdef1234567890", Some(Vec::new())),
        "abcdef123456"
    );
    assert_eq!(container_display_name("short", None), "short");
}

#[test]
fn container_app_name_includes_compose_project_service_and_container_name() {
    let labels = HashMap::from([
        (
            "com.docker.compose.project".to_string(),
            "cortex".to_string(),
        ),
        (
            "com.docker.compose.service".to_string(),
            "server".to_string(),
        ),
    ]);

    assert_eq!(
        container_app_name("cortex-1", &labels),
        "cortex/server/cortex-1"
    );
}

#[test]
fn container_app_name_falls_back_to_service_or_container_name() {
    let service_only = HashMap::from([(
        "com.docker.compose.service".to_string(),
        "server".to_string(),
    )]);

    assert_eq!(
        container_app_name("cortex-1", &service_only),
        "server/cortex-1"
    );
    assert_eq!(container_app_name("cortex-1", &HashMap::new()), "cortex-1");
}

#[test]
fn should_forward_container_logs_excludes_cortex_server_containers() {
    let compose_cortex = HashMap::from([
        (
            "com.docker.compose.project".to_string(),
            "cortex".to_string(),
        ),
        (
            "com.docker.compose.service".to_string(),
            "cortex".to_string(),
        ),
    ]);
    assert!(!should_forward_container_logs("cortex", &compose_cortex));

    let plain_cortex = HashMap::new();
    assert!(!should_forward_container_logs("cortex", &plain_cortex));

    let agent = HashMap::from([(
        "com.docker.compose.service".to_string(),
        "cortex-agent".to_string(),
    )]);
    assert!(should_forward_container_logs("cortex-agent", &agent));

    let unrelated = HashMap::from([
        ("com.docker.compose.project".to_string(), "lab".to_string()),
        (
            "com.docker.compose.service".to_string(),
            "labby".to_string(),
        ),
    ]);
    assert!(should_forward_container_logs("labby", &unrelated));
}

#[test]
fn connect_constructs_http_client_without_eager_network_io() {
    connect("http://127.0.0.1:2375").unwrap();
}

#[cfg(unix)]
#[test]
fn connect_constructs_unix_socket_client_without_eager_network_io() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("docker.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();

    connect(&format!("unix://{}", socket.display())).unwrap();
}
