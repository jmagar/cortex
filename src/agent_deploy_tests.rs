use super::*;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serial_test::serial;

struct EnvGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(name);
        unsafe { std::env::set_var(name, value) };
        Self { name, previous }
    }

    fn remove(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        unsafe { std::env::remove_var(name) };
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var(self.name, value) },
            None => unsafe { std::env::remove_var(self.name) },
        }
    }
}

fn write_executable(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

fn prepend_path(dir: &Path) -> EnvGuard {
    let previous = std::env::var_os("PATH").unwrap_or_default();
    let mut new_path = std::ffi::OsString::from(dir.as_os_str());
    new_path.push(":");
    new_path.push(previous);
    EnvGuard::set("PATH", new_path)
}

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

#[test]
#[serial]
fn deploy_syslog_target_prefers_nonblank_env_override() {
    let _guard = EnvGuard::set("CORTEX_SYSLOG_TARGET", "syslog.example.test:1514");

    assert_eq!(
        deploy_syslog_target(Some("https://cortex.example.test:3100")),
        Some("syslog.example.test:1514".to_string())
    );
}

#[test]
#[serial]
fn deploy_syslog_target_ignores_blank_env_and_falls_back_to_heartbeat() {
    let _guard = EnvGuard::set("CORTEX_SYSLOG_TARGET", "  ");

    assert_eq!(
        deploy_syslog_target(Some("https://cortex.example.test:3100/path")),
        Some("cortex.example.test:1514".to_string())
    );
}

#[test]
#[serial]
fn is_unraid_uses_ssh_probe_output() {
    let dir = tempfile::tempdir().unwrap();
    write_executable(
        &dir.path().join("ssh"),
        "#!/bin/sh\ncase \"$*\" in\n  *unraid-host*) printf 'yes\\n'; exit 0 ;;\n  *linux-host*) printf 'no\\n'; exit 0 ;;\n  *) exit 1 ;;\nesac\n",
    );
    let _path = prepend_path(dir.path());

    assert!(is_unraid("unraid-host"));
    assert!(!is_unraid("linux-host"));
    assert!(!is_unraid("unknown-host"));
}

#[test]
#[serial]
fn which_cortex_returns_nonempty_success_and_ignores_blank_or_failure() {
    let dir = tempfile::tempdir().unwrap();
    let cortex_path = PathBuf::from("/tmp/cortex-from-test");
    write_executable(
        &dir.path().join("which"),
        "#!/bin/sh\ncase \"$CORTEX_TEST_WHICH_MODE\" in\n  ok) printf '%s\\n' \"$CORTEX_TEST_CORTEX_PATH\"; exit 0 ;;\n  blank) printf '\\n'; exit 0 ;;\n  fail) exit 1 ;;\n  *) exit 2 ;;\nesac\n",
    );
    let _path = prepend_path(dir.path());
    let _cortex_path = EnvGuard::set("CORTEX_TEST_CORTEX_PATH", &cortex_path);

    {
        let _mode = EnvGuard::set("CORTEX_TEST_WHICH_MODE", "ok");
        assert_eq!(which_cortex(), Some(cortex_path.clone()));
    }
    {
        let _mode = EnvGuard::set("CORTEX_TEST_WHICH_MODE", "blank");
        assert_eq!(which_cortex(), None);
    }
    {
        let _mode = EnvGuard::set("CORTEX_TEST_WHICH_MODE", "fail");
        assert_eq!(which_cortex(), None);
    }
}

#[test]
#[serial]
fn find_local_binary_prefers_installed_cortex_from_path() {
    let dir = tempfile::tempdir().unwrap();
    let cortex_path = dir.path().join("cortex");
    write_executable(&cortex_path, "#!/bin/sh\nexit 0\n");
    write_executable(
        &dir.path().join("which"),
        "#!/bin/sh\n[ \"$1\" = cortex ] || exit 1\nprintf '%s\\n' \"$CORTEX_TEST_CORTEX_PATH\"\n",
    );
    let _path = prepend_path(dir.path());
    let _cortex_path = EnvGuard::set("CORTEX_TEST_CORTEX_PATH", &cortex_path);
    let _mode = EnvGuard::remove("CORTEX_TEST_WHICH_MODE");

    assert_eq!(find_local_binary(), Some(cortex_path));
}
