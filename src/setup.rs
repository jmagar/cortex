use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::Instant;

// `setup install` ships the *publishable* template — the local-dev
// `docker-compose.yml` extends this file to build from source.
const COMPOSE_ASSET: &str = include_str!("../docker-compose.prod.yml");
const DOCKERFILE_ASSET: &str = include_str!("../config/Dockerfile");

mod agent_command;
mod ai_index;
mod ai_watch;
mod debug_wrapper;
mod doctor;
mod firstrun;
mod systemd;

pub use agent_command::run_agent_command_setup;
pub use ai_index::run_ai_index_timer_setup;
pub use ai_watch::run_ai_watch_service_setup;
pub use debug_wrapper::{run_debug_compose_setup, run_debug_wrapper_setup};
pub use doctor::run_setup_doctor;
pub use firstrun::run_setup;
pub(crate) use firstrun::{
    default_env_for_data_dir, dockerfile_asset, installed_compose_asset, render_env,
};

// Test-only re-exports of private items accessed via `use super::*` in setup_tests.rs.
#[cfg(test)]
pub(crate) use ai_index::{ai_index_script, ai_index_service_unit, ai_index_timer_unit};
#[cfg(test)]
pub(crate) use ai_watch::{
    ai_index_output_status, ai_watch_env_file, ai_watch_service_unit,
    check_ai_watch_service_content_phase, summarize_ai_index_output,
    transcript_root_permissions_phase,
};
#[cfg(test)]
pub(crate) use debug_wrapper::{
    check_debug_compose_content_phase, check_debug_wrapper_content_phase, debug_compose_override,
    debug_wrapper_script,
};
#[cfg(test)]
pub(crate) use firstrun::{ensure_env_file, parse_env, write_env};
#[cfg(test)]
pub(crate) use systemd::inferred_user_bus_env;

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupMode {
    FirstRun,
    Check,
    Repair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiIndexTimerAction {
    Install,
    Remove,
    Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiWatchServiceAction {
    Install,
    Remove,
    Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCommandAction {
    Install,
    Remove,
    Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugWrapperAction {
    Install,
    Remove,
    Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugComposeAction {
    Install,
    Remove,
    Check,
}

impl DebugComposeAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "debug-compose-install",
            Self::Remove => "debug-compose-remove",
            Self::Check => "debug-compose-check",
        }
    }
}

impl DebugWrapperAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "debug-wrapper-install",
            Self::Remove => "debug-wrapper-remove",
            Self::Check => "debug-wrapper-check",
        }
    }
}

impl AiWatchServiceAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "ai-watch-service-install",
            Self::Remove => "ai-watch-service-remove",
            Self::Check => "ai-watch-service-check",
        }
    }
}

impl AgentCommandAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "agent-command-install",
            Self::Remove => "agent-command-remove",
            Self::Check => "agent-command-check",
        }
    }
}

impl AiIndexTimerAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "ai-index-timer-install",
            Self::Remove => "ai-index-timer-remove",
            Self::Check => "ai-index-timer-check",
        }
    }
}

impl SetupMode {
    fn mutates(self) -> bool {
        !matches!(self, Self::Check)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::FirstRun => "first-run",
            Self::Check => "check",
            Self::Repair => "repair",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    Ok,
    Warn,
    Error,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupIssueKind {
    BlockingError,
    DataQualityWarning,
    RuntimeState,
    FileState,
    Command,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupPhase {
    pub name: &'static str,
    pub status: SetupStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_kind: Option<SetupIssueKind>,
    pub detail: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupReport {
    pub mode: &'static str,
    pub elapsed_ms: u128,
    pub home: PathBuf,
    pub env_path: PathBuf,
    pub compose_dir: PathBuf,
    pub data_dir: PathBuf,
    pub health_url: String,
    pub mcp_url: String,
    pub phases: Vec<SetupPhase>,
    pub has_errors: bool,
    pub blocking_errors: usize,
    pub data_quality_warnings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watcher_healthy: Option<bool>,
}

struct SetupReportInput {
    mode: &'static str,
    elapsed_ms: u128,
    home: PathBuf,
    env_path: PathBuf,
    compose_dir: PathBuf,
    data_dir: PathBuf,
    health_url: String,
    mcp_url: String,
}

pub(crate) struct PhaseTimer {
    name: &'static str,
    start: Instant,
}

impl PhaseTimer {
    pub(crate) fn start(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }

    pub(crate) fn finish(self, status: SetupStatus, detail: impl Into<String>) -> SetupPhase {
        self.finish_with_issue(status, None, detail)
    }

    pub(crate) fn finish_with_issue(
        self,
        status: SetupStatus,
        issue_kind: Option<SetupIssueKind>,
        detail: impl Into<String>,
    ) -> SetupPhase {
        SetupPhase {
            name: self.name,
            status,
            issue_kind,
            detail: detail.into(),
            elapsed_ms: self.start.elapsed().as_millis(),
        }
    }
}

fn phases_have_errors(phases: &[SetupPhase]) -> bool {
    phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error))
}

fn report_summary(phases: &[SetupPhase]) -> (bool, usize, usize) {
    let blocking_errors = phases
        .iter()
        .filter(|phase| matches!(phase.status, SetupStatus::Error))
        .count();
    let data_quality_warnings = phases
        .iter()
        .filter(|phase| matches!(phase.issue_kind, Some(SetupIssueKind::DataQualityWarning)))
        .count();
    (blocking_errors > 0, blocking_errors, data_quality_warnings)
}

fn ai_watch_service_state(phases: &[SetupPhase]) -> (Option<bool>, Option<bool>) {
    let service_enabled = phase_is_ok(phases, AI_WATCH_SERVICE_ENABLED_PHASE);
    let watcher_healthy = phase_is_ok(phases, AI_WATCH_SERVICE_ACTIVE_PHASE);
    (service_enabled, watcher_healthy)
}

fn phase_is_ok(phases: &[SetupPhase], name: &str) -> Option<bool> {
    phases
        .iter()
        .find(|phase| phase.name == name)
        .map(|phase| matches!(phase.status, SetupStatus::Ok))
}

fn should_skip_ai_watch_systemd_enable(phases: &[SetupPhase]) -> bool {
    phases_have_errors(phases)
}

fn skipped_phase(name: &'static str, detail: impl Into<String>) -> SetupPhase {
    SetupPhase {
        name,
        status: SetupStatus::Skipped,
        issue_kind: None,
        detail: detail.into(),
        elapsed_ms: 0,
    }
}

const AI_WATCH_SERVICE_ENABLED_PHASE: &str = "ai-watch-service-enabled";
const AI_WATCH_SERVICE_ACTIVE_PHASE: &str = "ai-watch-service-active";

fn setup_report(input: SetupReportInput, phases: Vec<SetupPhase>) -> SetupReport {
    let (has_errors, blocking_errors, data_quality_warnings) = report_summary(&phases);
    let (service_enabled, watcher_healthy) = ai_watch_service_state(&phases);
    SetupReport {
        mode: input.mode,
        elapsed_ms: input.elapsed_ms,
        home: input.home,
        env_path: input.env_path,
        compose_dir: input.compose_dir,
        data_dir: input.data_dir,
        health_url: input.health_url,
        mcp_url: input.mcp_url,
        phases,
        has_errors,
        blocking_errors,
        data_quality_warnings,
        service_enabled,
        watcher_healthy,
    }
}

fn host_local_report_input(
    mode: &'static str,
    elapsed_ms: u128,
    home: PathBuf,
    env_path: PathBuf,
    compose_dir: PathBuf,
    data_dir: PathBuf,
) -> SetupReportInput {
    SetupReportInput {
        mode,
        elapsed_ms,
        home,
        env_path,
        compose_dir,
        data_dir,
        health_url: "host-local helper".to_string(),
        mcp_url: "host-local helper".to_string(),
    }
}

fn check_file_phase(name: &'static str, path: &Path, fix: &str) -> SetupPhase {
    let timer = PhaseTimer::start(name);
    if path.exists() {
        timer.finish(SetupStatus::Ok, format!("found {}", path.display()))
    } else {
        timer.finish(
            SetupStatus::Warn,
            format!("missing {}; {fix}", path.display()),
        )
    }
}

fn setup_path_value(path: &Path) -> io::Result<String> {
    let raw = path.display().to_string();
    if raw.is_empty()
        || raw.chars().any(|ch| {
            ch.is_control() || ch.is_whitespace() || matches!(ch, '"' | '\'' | '%' | '\\')
        })
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unsupported character in setup path: {raw}"),
        ));
    }
    Ok(raw)
}

pub(crate) struct EnvResult {
    pub(crate) phase: SetupPhase,
    pub(crate) values: BTreeMap<String, String>,
}

pub fn syslog_home_dir() -> io::Result<PathBuf> {
    if let Ok(value) = std::env::var("SYSLOG_MCP_HOME") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return validate_absolute_home(PathBuf::from(trimmed));
        }
    }
    let home =
        std::env::var("HOME").map_err(|_| io::Error::new(ErrorKind::NotFound, "HOME is unset"))?;
    validate_absolute_home(PathBuf::from(home).join(".syslog-mcp"))
}

fn user_home_dir() -> io::Result<PathBuf> {
    let home =
        std::env::var("HOME").map_err(|_| io::Error::new(ErrorKind::NotFound, "HOME is unset"))?;
    let home = PathBuf::from(home);
    setup_path_value(&home)?;
    Ok(home)
}

fn validate_absolute_home(path: PathBuf) -> io::Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "setup home must be an absolute path without '..': {}",
                path.display()
            ),
        ));
    }
    Ok(path)
}

fn resolve_syslog_binary() -> io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    let output = std::process::Command::new("sh")
        .args(["-c", "command -v syslog"])
        .output()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return validate_executable_path(PathBuf::from(path));
        }
    }
    if current.file_name().and_then(|name| name.to_str()) == Some("syslog") {
        return validate_executable_path(current);
    }
    Err(io::Error::new(
        ErrorKind::NotFound,
        "syslog binary not found on PATH",
    ))
}

fn validate_executable_path(path: PathBuf) -> io::Result<PathBuf> {
    let canonical = path.canonicalize()?;
    setup_path_value(&canonical)?;
    if !allow_debug_binary() && looks_like_debug_build_path(&canonical) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "refusing to install AI watch service with debug/worktree binary {}; put the syslog wrapper on PATH or set SYSLOG_AI_WATCH_ALLOW_DEBUG_BINARY=true",
                canonical.display()
            ),
        ));
    }
    let metadata = std::fs::metadata(&canonical)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("not a file: {}", canonical.display()),
        ));
    }
    Ok(canonical)
}

fn allow_debug_binary() -> bool {
    std::env::var("SYSLOG_AI_WATCH_ALLOW_DEBUG_BINARY")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn looks_like_debug_build_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.contains("/target/debug/") || text.contains("/.cache/cargo/debug/")
}

fn resolve_ai_watch_db_path(setup_home: &Path, user_home: &Path) -> io::Result<PathBuf> {
    if let Ok(value) = std::env::var("SYSLOG_MCP_DB_PATH") {
        if !value.trim().is_empty() {
            return validate_db_path(PathBuf::from(value));
        }
    }
    if let Some(path) = db_path_from_setup_env(&setup_home.join(".env"))? {
        return validate_db_path(path);
    }
    let plugin_db = user_home.join(".claude/plugins/data/syslog-jmagar-lab/syslog.db");
    if plugin_db.exists() {
        return validate_db_path(plugin_db);
    }
    validate_db_path(setup_home.join("data/syslog.db"))
}

fn validate_db_path(path: PathBuf) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("AI watch DB path must be absolute: {}", path.display()),
        ));
    }
    setup_path_value(&path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let Some(parent) = parent.filter(|parent| *parent != Path::new("/")) else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "AI watch DB path must live under a non-root directory: {}",
                path.display()
            ),
        ));
    };
    std::fs::create_dir_all(parent)?;
    Ok(path)
}

fn db_path_from_setup_env(env_path: &Path) -> io::Result<Option<PathBuf>> {
    let raw = match std::fs::read_to_string(env_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let values = firstrun::parse_env(&raw);
    if let Some(db_path) = values.get("SYSLOG_MCP_DB_PATH") {
        if !db_path.trim().is_empty() && db_path != "/data/syslog.db" {
            return Ok(Some(PathBuf::from(db_path)));
        }
    }
    let uses_container_db_path = values
        .get("SYSLOG_MCP_DB_PATH")
        .is_some_and(|db_path| db_path == "/data/syslog.db");
    let Some(data_volume) = values.get("SYSLOG_MCP_DATA_VOLUME") else {
        if uses_container_db_path {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "{} uses SYSLOG_MCP_DB_PATH=/data/syslog.db but does not set absolute SYSLOG_MCP_DATA_VOLUME",
                    env_path.display()
                ),
            ));
        }
        return Ok(None);
    };
    let volume_path = PathBuf::from(data_volume);
    if volume_path.is_absolute() {
        return Ok(Some(volume_path.join("syslog.db")));
    }
    if uses_container_db_path {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "{} uses SYSLOG_MCP_DB_PATH=/data/syslog.db but SYSLOG_MCP_DATA_VOLUME is not absolute: {}",
                env_path.display(),
                data_volume
            ),
        ));
    }
    Ok(None)
}

fn current_uid_gid() -> (String, String) {
    firstrun::current_uid_gid()
}

pub(crate) fn write_executable_file(path: &Path, body: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut options = std::fs::OpenOptions::new();
        options
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o755)
            .custom_flags(libc::O_NOFOLLOW);
        options.open(path)?.write_all(body.as_bytes())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    std::fs::write(path, body)?;
    Ok(())
}

pub(crate) fn write_private_file(path: &Path, body: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut options = std::fs::OpenOptions::new();
        options
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW);
        options.open(path)?.write_all(body.as_bytes())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    std::fs::write(path, body)?;
    Ok(())
}
