use super::systemd::systemctl_user_state;
use super::{PhaseTimer, SetupPhase, SetupStatus};

const LEGACY_AI_SYSTEMD_UNITS: &[&str] = &["cortex-ai-watch.service", "cortex-ai-index.timer"];

pub(crate) fn legacy_ai_systemd_units_absent_phase() -> SetupPhase {
    let timer = PhaseTimer::start("legacy-ai-systemd-units-absent");
    let stale = LEGACY_AI_SYSTEMD_UNITS
        .iter()
        .filter_map(|unit| {
            let active = systemctl_user_state("is-active", unit);
            let enabled = systemctl_user_state("is-enabled", unit);
            let active_stale = active.as_deref() == Some("active");
            let enabled_stale = enabled.as_deref() == Some("enabled");
            (active_stale || enabled_stale)
                .then(|| format!("{unit} active={active:?} enabled={enabled:?}"))
        })
        .collect::<Vec<_>>();
    if stale.is_empty() {
        return timer.finish(
            SetupStatus::Ok,
            "legacy cortex-ai systemd units inactive or absent",
        );
    }
    timer.finish(
        SetupStatus::Error,
        format!(
            "legacy cortex-ai systemd units still active/enabled: {}; run `systemctl --user disable --now {}`",
            stale.join("; "),
            LEGACY_AI_SYSTEMD_UNITS.join(" ")
        ),
    )
}

pub(crate) fn ai_index_timer_disabled_phase() -> SetupPhase {
    let timer = PhaseTimer::start("sessions-index-timer-disabled");
    let active = systemctl_user_state("is-active", "cortex-sessions-index.timer");
    let enabled = systemctl_user_state("is-enabled", "cortex-sessions-index.timer");
    if active.as_deref() == Some("active") || enabled.as_deref() == Some("enabled") {
        return timer.finish(
            SetupStatus::Error,
            format!(
                "cortex-sessions-index.timer still active/enabled (active={active:?}, enabled={enabled:?})"
            ),
        );
    }
    timer.finish(
        SetupStatus::Ok,
        format!(
            "cortex-sessions-index.timer inactive or absent (active={active:?}, enabled={enabled:?})"
        ),
    )
}

#[cfg(test)]
#[path = "sessions_watch_legacy_tests.rs"]
mod tests;
