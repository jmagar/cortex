use std::io::{self, ErrorKind};
use std::path::Path;

use super::systemd::{systemctl_user_phase, systemctl_user_required_phase, systemctl_user_state};
use super::{PhaseTimer, SetupPhase, SetupStatus};

/// One named health condition checked by the periodic sessions-watch
/// doctor alert. `unhealthy=true` means this condition should trigger a
/// notification. Bead .2 (route-scoped CORTEX_INGEST_TOKEN) appends its own
/// "using CORTEX_API_TOKEN fallback" condition to the Vec this function
/// returns — do not rename the fields without updating that bead's plan.
#[derive(Debug, Clone)]
pub(crate) struct HealthCondition {
    pub name: &'static str,
    pub unhealthy: bool,
    pub detail: String,
}

/// Collect all health conditions relevant to `cortex-sessions-watch.service`.
/// Returns one entry per condition regardless of health, so callers can log
/// or notify on the full set, not just the unhealthy ones.
pub(crate) fn sessions_watch_health_conditions() -> Vec<HealthCondition> {
    let active = systemctl_user_state("is-active", "cortex-sessions-watch.service");
    let is_failed = active.as_deref() == Some("failed");
    vec![HealthCondition {
        name: "sessions-watch-service-failed",
        unhealthy: is_failed,
        detail: format!("cortex-sessions-watch.service is-active={active:?}"),
    }]
}

/// Check all sessions-watch health conditions; if any are unhealthy, send
/// one Apprise notification summarizing them. Returns a SetupPhase so this
/// composes with the rest of the setup-report machinery (and so `cortex
/// setup doctor` can surface it alongside other checks).
pub(crate) async fn run_sessions_watch_health_check_and_notify(
    apprise_base_url: &str,
    apprise_urls: &[String],
) -> SetupPhase {
    let timer = PhaseTimer::start("sessions-watch-health-check");
    let conditions = sessions_watch_health_conditions();
    let unhealthy: Vec<&HealthCondition> = conditions.iter().filter(|c| c.unhealthy).collect();

    if unhealthy.is_empty() {
        return timer.finish(
            SetupStatus::Ok,
            "all sessions-watch health conditions healthy",
        );
    }

    let body = unhealthy
        .iter()
        .map(|c| format!("- {}: {}", c.name, c.detail))
        .collect::<Vec<_>>()
        .join("\n");

    if !apprise_urls.is_empty() {
        let client = crate::notifications::apprise::AppriseClient::new(apprise_base_url);
        if let Err(error) = client
            .notify(
                apprise_urls,
                "cortex sessions-watch unhealthy",
                &body,
                crate::notifications::apprise::NotifyType::Warning,
            )
            .await
        {
            tracing::warn!(error = %error, "sessions-watch health alert: Apprise notify failed");
        }
    }

    timer.finish(
        SetupStatus::Error,
        format!("unhealthy conditions detected:\n{body}"),
    )
}

fn install_health_check_timer_files(systemd_dir: &Path) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("sessions-watch-doctor-timer-files");
    let service_content = include_str!("../../config/systemd/cortex-sessions-watch-doctor.service");
    let timer_content = include_str!("../../config/systemd/cortex-sessions-watch-doctor.timer");
    std::fs::create_dir_all(systemd_dir)?;
    std::fs::write(
        systemd_dir.join("cortex-sessions-watch-doctor.service"),
        service_content,
    )?;
    std::fs::write(
        systemd_dir.join("cortex-sessions-watch-doctor.timer"),
        timer_content,
    )?;
    Ok(timer.finish(
        SetupStatus::Ok,
        "wrote cortex-sessions-watch-doctor service+timer",
    ))
}

/// Write the doctor timer/service files and enable the timer. Called from
/// `SessionsWatchServiceAction::Install` after the watch service itself is
/// enabled+active.
pub(super) fn install_and_enable_doctor_timer(systemd_dir: &Path) -> io::Result<Vec<SetupPhase>> {
    Ok(vec![
        install_health_check_timer_files(systemd_dir)?,
        systemctl_user_phase(&["daemon-reload"]),
        systemctl_user_required_phase(&["enable", "--now", "cortex-sessions-watch-doctor.timer"]),
    ])
}

/// Disable the doctor timer and remove its files. Called from
/// `SessionsWatchServiceAction::Remove`.
pub(super) fn disable_and_remove_doctor_timer(systemd_dir: &Path) -> io::Result<Vec<SetupPhase>> {
    Ok(vec![
        systemctl_user_phase(&["disable", "--now", "cortex-sessions-watch-doctor.timer"]),
        remove_health_check_timer_files(systemd_dir)?,
    ])
}

/// Resolve notification config and run the health check + alert. Called
/// from `SessionsWatchServiceAction::HealthCheck`.
pub(super) async fn build_and_run_health_check() -> SetupPhase {
    let notifications = crate::config::Config::load()
        .map(|config| config.notifications)
        .unwrap_or_default();
    let apprise_urls: Vec<String> = if notifications.enabled {
        notifications.apprise_urls.clone()
    } else {
        Vec::new()
    };
    run_sessions_watch_health_check_and_notify(&notifications.apprise_url, &apprise_urls).await
}

fn remove_health_check_timer_files(systemd_dir: &Path) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("sessions-watch-doctor-timer-files");
    for name in [
        "cortex-sessions-watch-doctor.service",
        "cortex-sessions-watch-doctor.timer",
    ] {
        match std::fs::remove_file(systemd_dir.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(timer.finish(
        SetupStatus::Ok,
        "removed cortex-sessions-watch-doctor service+timer files",
    ))
}

#[cfg(test)]
#[path = "sessions_watch_health_tests.rs"]
mod tests;
