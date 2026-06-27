use super::*;

#[test]
fn user_bus_env_for_runtime_dir_returns_none_when_bus_socket_is_absent() {
    let tmp = tempfile::tempdir().unwrap();

    assert_eq!(user_bus_env_for_runtime_dir(tmp.path()), None);
}

#[test]
fn user_bus_env_for_runtime_dir_formats_dbus_socket_path() {
    let tmp = tempfile::tempdir().unwrap();
    let bus = tmp.path().join("bus");
    std::fs::write(&bus, "").unwrap();

    let (runtime_dir, bus_address) = user_bus_env_for_runtime_dir(tmp.path()).unwrap();

    assert_eq!(runtime_dir, tmp.path());
    assert_eq!(bus_address, format!("unix:path={}", bus.display()));
}

#[test]
fn current_uid_is_available_for_system_adapter_env_inference() {
    let uid = current_uid();

    #[cfg(unix)]
    assert_eq!(uid, unsafe { libc::geteuid() });
    #[cfg(not(unix))]
    assert_eq!(uid, 0);
}
