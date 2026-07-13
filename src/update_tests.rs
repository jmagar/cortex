use super::*;

#[test]
fn update_profile_round_trips_server_and_clients() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployments.toml");
    let profile = UpdateProfile {
        server: Some(ServerUpdateProfile {
            host: "tootie".to_string(),
            home: "/mnt/cache/appdata/cortex".to_string(),
        }),
        clients: ClientsUpdateProfile {
            hosts: vec!["dookie".to_string(), "shart".to_string()],
            target: Some("https://cortex.tootie.tv".to_string()),
            docker: Some(true),
            journald: None,
        },
    };

    write_profile(&path, &profile).unwrap();
    let loaded = load_profile(&path).unwrap();

    assert_eq!(loaded, profile);
}

#[test]
fn configure_server_profile_validates_and_preserves_clients() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployments.toml");
    write_profile(
        &path,
        &UpdateProfile {
            server: None,
            clients: ClientsUpdateProfile {
                hosts: vec!["dookie".to_string()],
                target: Some("https://cortex.tootie.tv".to_string()),
                docker: Some(true),
                journald: Some(false),
            },
        },
    )
    .unwrap();

    let updated =
        configure_server_profile(Some(&path), "tootie", "/mnt/cache/appdata/cortex").unwrap();

    assert_eq!(updated.server.as_ref().unwrap().host, "tootie");
    assert_eq!(
        updated.server.as_ref().unwrap().home,
        "/mnt/cache/appdata/cortex"
    );
    assert_eq!(updated.clients.hosts, vec!["dookie"]);
}

#[test]
fn configure_server_profile_rejects_unsafe_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployments.toml");

    let bad_host = configure_server_profile(
        Some(&path),
        "-oProxyCommand=touch /tmp/pwned",
        "/mnt/cache/appdata/cortex",
    )
    .unwrap_err();
    assert!(bad_host.to_string().contains("unsafe ssh host"));

    let bad_home = configure_server_profile(Some(&path), "tootie", "relative/path").unwrap_err();
    assert!(bad_home.to_string().contains("absolute path"));

    let parent_home =
        configure_server_profile(Some(&path), "tootie", "/mnt/cache/../cortex").unwrap_err();
    assert!(parent_home.to_string().contains("must not contain '..'"));
}
