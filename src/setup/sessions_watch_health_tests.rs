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
#[test]
#[serial]
fn sessions_watch_health_conditions_flags_failed_service() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("systemctl"),
        "#!/bin/sh\ncase \"$*\" in\n  *is-active*cortex-sessions-watch.service*) printf 'failed\\n' ;;\n  *) printf 'inactive\\n' ;;\nesac\nexit 0\n",
    );
    let _path = EnvGuard::set("PATH", path_with_prepended(&bin_dir));

    let conditions = sessions_watch_health_conditions();

    let watch_condition = conditions
        .iter()
        .find(|c| c.name == "sessions-watch-service-failed")
        .expect("sessions-watch-service-failed condition present");
    assert!(
        watch_condition.unhealthy,
        "expected unhealthy: {watch_condition:?}"
    );
    assert!(watch_condition.detail.contains("failed"));
}

#[cfg(unix)]
#[test]
#[serial]
fn sessions_watch_health_conditions_reports_healthy_when_active() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("systemctl"),
        "#!/bin/sh\ncase \"$*\" in\n  *is-active*cortex-sessions-watch.service*) printf 'active\\n' ;;\n  *) printf 'inactive\\n' ;;\nesac\nexit 0\n",
    );
    let _path = EnvGuard::set("PATH", path_with_prepended(&bin_dir));

    let conditions = sessions_watch_health_conditions();

    let watch_condition = conditions
        .iter()
        .find(|c| c.name == "sessions-watch-service-failed")
        .expect("sessions-watch-service-failed condition present");
    assert!(
        !watch_condition.unhealthy,
        "expected healthy: {watch_condition:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn health_check_and_notify_sends_apprise_alert_when_unhealthy() {
    use axum::{Router, routing::post};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("systemctl"),
        "#!/bin/sh\ncase \"$*\" in\n  *is-active*cortex-sessions-watch.service*) printf 'failed\\n' ;;\n  *) printf 'inactive\\n' ;;\nesac\nexit 0\n",
    );
    let _path = EnvGuard::set("PATH", path_with_prepended(&bin_dir));

    let notified = Arc::new(AtomicBool::new(false));
    let notified_clone = notified.clone();
    let app = Router::new().route(
        "/notify/",
        post(move || {
            let notified = notified_clone.clone();
            async move {
                notified.store(true, Ordering::SeqCst);
                axum::http::StatusCode::OK
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{addr}");
    let phase = run_sessions_watch_health_check_and_notify(
        &base_url,
        &["gotify://example.invalid/token".to_string()],
    )
    .await;

    assert_eq!(phase.status, SetupStatus::Error);
    assert!(
        notified.load(Ordering::SeqCst),
        "expected Apprise notify to fire"
    );
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn health_check_and_notify_skips_apprise_when_healthy() {
    use axum::{Router, routing::post};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("systemctl"),
        "#!/bin/sh\ncase \"$*\" in\n  *is-active*cortex-sessions-watch.service*) printf 'active\\n' ;;\n  *) printf 'inactive\\n' ;;\nesac\nexit 0\n",
    );
    let _path = EnvGuard::set("PATH", path_with_prepended(&bin_dir));

    let notified = Arc::new(AtomicBool::new(false));
    let notified_clone = notified.clone();
    let app = Router::new().route(
        "/notify/",
        post(move || {
            let notified = notified_clone.clone();
            async move {
                notified.store(true, Ordering::SeqCst);
                axum::http::StatusCode::OK
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{addr}");
    let phase = run_sessions_watch_health_check_and_notify(
        &base_url,
        &["gotify://example.invalid/token".to_string()],
    )
    .await;

    assert_eq!(phase.status, SetupStatus::Ok);
    assert!(
        !notified.load(Ordering::SeqCst),
        "expected no Apprise notify when healthy"
    );
}
