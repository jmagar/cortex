use super::*;
use serial_test::serial;

struct EnvGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(name);
        unsafe {
            std::env::set_var(name, value);
        }
        Self { name, previous }
    }

    fn remove(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        unsafe {
            std::env::remove_var(name);
        }
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe {
                std::env::set_var(self.name, value);
            },
            None => unsafe {
                std::env::remove_var(self.name);
            },
        }
    }
}

#[cfg(unix)]
fn write_executable(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, body).unwrap();
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

fn path_with_prepended(dir: &std::path::Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).unwrap()
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn run_ai_watch_service_setup_install_check_and_remove_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let cortex_home = home.join(".cortex");
    let bin_dir = dir.path().join("bin");
    let db_path = cortex_home.join("data/cortex.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(home.join(".claude/projects")).unwrap();
    std::fs::create_dir_all(home.join(".codex/sessions")).unwrap();
    std::fs::create_dir_all(home.join(".gemini/tmp")).unwrap();
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("cortex"),
        "#!/bin/sh\nif [ \"$1 $2\" = \"ai index\" ]; then printf '{\"discovered_files\":2,\"ingested\":1,\"skipped_dupes\":1,\"parse_errors\":0,\"storage_blocked_chunks\":0,\"dropped_metadata_fields\":0,\"file_errors\":[]}\\n'; exit 0; fi\nexit 0\n",
    );
    write_executable(
        &bin_dir.join("systemctl"),
        "#!/bin/sh\ncase \"$*\" in\n  *is-active*cortex-ai-index.timer*) printf 'inactive\\n' ;;\n  *is-enabled*cortex-ai-index.timer*) printf 'disabled\\n' ;;\n  *is-enabled*cortex-ai-watch.service*) printf 'enabled\\n' ;;\n  *is-active*cortex-ai-watch.service*) printf 'active\\n' ;;\n  *) printf 'ok\\n' ;;\nesac\nexit 0\n",
    );

    let _home = EnvGuard::set("HOME", &home);
    let _cortex_home = EnvGuard::set("CORTEX_HOME", &cortex_home);
    let _db_path = EnvGuard::set("CORTEX_DB_PATH", &db_path);
    let _path = EnvGuard::set("PATH", path_with_prepended(&bin_dir));

    let install = run_ai_watch_service_setup(AiWatchServiceAction::Install)
        .await
        .unwrap();
    assert_eq!(install.mode, "ai-watch-service-install");
    assert!(
        install
            .phases
            .iter()
            .any(|phase| phase.name == "ai-watch-service-files" && phase.status == SetupStatus::Ok)
    );
    assert!(
        install
            .phases
            .iter()
            .any(|phase| phase.name == "ai-watch-initial-index" && phase.status == SetupStatus::Ok)
    );
    assert!(
        install
            .phases
            .iter()
            .any(|phase| phase.name == AI_WATCH_SERVICE_ACTIVE_PHASE && phase.detail == "active")
    );
    let env_path = home.join(".config/cortex/ai-watch.env");
    let service_path = home.join(".config/systemd/user/cortex-ai-watch.service");
    assert!(env_path.is_file());
    assert!(service_path.is_file());
    let service = std::fs::read_to_string(&service_path).unwrap();
    assert!(service.contains("ExecStart="));
    assert!(service.contains("ai watch --no-initial-scan --json"));
    assert!(service.contains("ProtectHome=read-only"));
    assert!(service.contains(".gemini/tmp"));

    let check = run_ai_watch_service_setup(AiWatchServiceAction::Check)
        .await
        .unwrap();
    assert_eq!(check.mode, "ai-watch-service-check");
    assert!(check.phases.iter().any(|phase| {
        phase.name == "ai-watch-service-content" && phase.status == SetupStatus::Ok
    }));
    assert!(
        check
            .phases
            .iter()
            .any(|phase| phase.name == AI_WATCH_SERVICE_ENABLED_PHASE && phase.detail == "enabled")
    );

    let remove = run_ai_watch_service_setup(AiWatchServiceAction::Remove)
        .await
        .unwrap();
    assert_eq!(remove.mode, "ai-watch-service-remove");
    assert!(!env_path.exists());
    assert!(!service_path.exists());
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn run_ai_watch_service_setup_rejects_relative_db_path_before_writing_files() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let cortex_home = home.join(".cortex");
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&cortex_home).unwrap();
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_executable(&bin_dir.join("cortex"), "#!/bin/sh\nexit 0\n");

    let _home = EnvGuard::set("HOME", &home);
    let _cortex_home = EnvGuard::set("CORTEX_HOME", &cortex_home);
    let _db_path = EnvGuard::set("CORTEX_DB_PATH", "relative/cortex.db");
    let _path = EnvGuard::set("PATH", path_with_prepended(&bin_dir));

    let error = run_ai_watch_service_setup(AiWatchServiceAction::Install)
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert!(error.to_string().contains("must be absolute"));
    assert!(!home.join(".config/cortex/ai-watch.env").exists());
}

#[test]
#[serial]
fn ai_watch_db_path_resolution_prefers_plugin_db_when_no_env_is_set() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let cortex_home = home.join(".cortex");
    let plugin_db = home.join(".claude/plugins/data/syslog-jmagar-lab/cortex.db");
    std::fs::create_dir_all(plugin_db.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&cortex_home).unwrap();
    std::fs::write(&plugin_db, "").unwrap();

    let _db_path = EnvGuard::remove("CORTEX_DB_PATH");

    let resolved = super::super::resolve_ai_watch_db_path(&cortex_home, &home).unwrap();

    assert_eq!(resolved, plugin_db);
}
