use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{self, ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const COMPOSE_ASSET: &str = include_str!("../docker-compose.yml");
const DOCKERFILE_ASSET: &str = include_str!("../config/Dockerfile");

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

impl AiWatchServiceAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "ai-watch-service-install",
            Self::Remove => "ai-watch-service-remove",
            Self::Check => "ai-watch-service-check",
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    Ok,
    Warn,
    Error,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupPhase {
    pub name: &'static str,
    pub status: SetupStatus,
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
}

struct PhaseTimer {
    name: &'static str,
    start: Instant,
}

impl PhaseTimer {
    fn start(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }

    fn finish(self, status: SetupStatus, detail: impl Into<String>) -> SetupPhase {
        SetupPhase {
            name: self.name,
            status,
            detail: detail.into(),
            elapsed_ms: self.start.elapsed().as_millis(),
        }
    }
}

pub async fn run_setup(mode: SetupMode) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = syslog_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let mut phases = Vec::new();

    phases.push(filesystem_phase(mode, &home, &data_dir, &compose_dir)?);
    let env = if mode.mutates() {
        let env_result = ensure_env_file(&env_path, &data_dir)?;
        phases.push(env_result.phase);
        phases.push(write_compose_assets(&compose_dir)?);
        Some(env_result.values)
    } else {
        phases.push(check_file_phase("env", &env_path, "run syslog setup"));
        phases.push(check_file_phase(
            "compose-assets",
            &compose_dir.join("docker-compose.yml"),
            "run syslog setup repair",
        ));
        None
    };

    phases.push(command_phase("docker", ["--version"]));
    phases.push(command_phase("docker compose", ["compose", "version"]));
    if mode.mutates() {
        phases.push(cleanup_legacy_systemd());
    }

    let prereq_failed = phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error));
    if mode.mutates() && !prereq_failed {
        ensure_network_phase(&mut phases, env.as_ref());
        phases.push(run_compose_phase(
            &compose_dir,
            &env_path,
            &["pull", "--ignore-buildable"],
        ));
        let up_args: &[&str] = if compose_dir.join("docker-compose.override.yml").exists() {
            &["up", "-d", "--build"]
        } else {
            &["up", "-d"]
        };
        phases.push(run_compose_phase(&compose_dir, &env_path, up_args));
        phases.push(health_phase(&env));
    } else {
        phases.push(SetupPhase {
            name: "compose-up",
            status: SetupStatus::Skipped,
            detail: if prereq_failed {
                "skipped because earlier checks failed".to_string()
            } else {
                "check mode does not start Docker services".to_string()
            },
            elapsed_ms: 0,
        });
    }

    let elapsed_ms = started.elapsed().as_millis();
    let has_errors = phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error));
    let port = env
        .as_ref()
        .and_then(|values| values.get("SYSLOG_MCP_PORT"))
        .cloned()
        .unwrap_or_else(|| "3100".to_string());
    Ok(SetupReport {
        mode: mode.as_str(),
        elapsed_ms,
        home,
        env_path,
        compose_dir,
        data_dir,
        health_url: format!("http://127.0.0.1:{port}/health"),
        mcp_url: format!("http://127.0.0.1:{port}/mcp"),
        phases,
        has_errors,
    })
}

pub async fn run_ai_index_timer_setup(action: AiIndexTimerAction) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = syslog_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = user_home_dir()?;
    let bin_path = user_home.join(".local/bin/syslog-ai-index");
    let systemd_dir = user_home.join(".config/systemd/user");
    let service_path = systemd_dir.join("syslog-ai-index.service");
    let timer_path = systemd_dir.join("syslog-ai-index.timer");
    let mut phases = Vec::new();

    match action {
        AiIndexTimerAction::Install => {
            phases.push(install_ai_index_timer_files(
                &bin_path,
                &systemd_dir,
                &service_path,
                &timer_path,
            )?);
            phases.push(systemctl_user_required_phase(&["daemon-reload"]));
            phases.push(systemctl_user_required_phase(&[
                "enable",
                "--now",
                "syslog-ai-index.timer",
            ]));
        }
        AiIndexTimerAction::Remove => {
            phases.push(systemctl_user_phase(&[
                "disable",
                "--now",
                "syslog-ai-index.timer",
            ]));
            phases.push(remove_ai_index_timer_files(
                &bin_path,
                &service_path,
                &timer_path,
            )?);
            phases.push(systemctl_user_phase(&["daemon-reload"]));
        }
        AiIndexTimerAction::Check => {
            phases.push(check_file_phase(
                "ai-index-bin",
                &bin_path,
                "run syslog setup ai-index-timer install",
            ));
            phases.push(check_file_phase(
                "ai-index-service",
                &service_path,
                "run syslog setup ai-index-timer install",
            ));
            phases.push(check_file_phase(
                "ai-index-timer",
                &timer_path,
                "run syslog setup ai-index-timer install",
            ));
            phases.push(systemctl_user_phase(&[
                "is-enabled",
                "syslog-ai-index.timer",
            ]));
        }
    }

    let elapsed_ms = started.elapsed().as_millis();
    let has_errors = phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error));
    Ok(SetupReport {
        mode: action.as_str(),
        elapsed_ms,
        home,
        env_path,
        compose_dir,
        data_dir,
        health_url: "host-local helper".to_string(),
        mcp_url: "host-local helper".to_string(),
        phases,
        has_errors,
    })
}

pub async fn run_ai_watch_service_setup(action: AiWatchServiceAction) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = syslog_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = user_home_dir()?;
    let config_dir = user_home.join(".config/syslog-mcp");
    let watch_env_path = config_dir.join("ai-watch.env");
    let state_dir = user_home.join(".local/state/syslog-mcp");
    let systemd_dir = user_home.join(".config/systemd/user");
    let service_path = systemd_dir.join("syslog-ai-watch.service");
    let mut phases = Vec::new();

    match action {
        AiWatchServiceAction::Install => {
            let syslog_bin = resolve_syslog_binary()?;
            let db_path = resolve_ai_watch_db_path(&home, &user_home)?;
            phases.push(install_ai_watch_service_files(
                &watch_env_path,
                &service_path,
                &systemd_dir,
                &state_dir,
                &syslog_bin,
                &db_path,
                &user_home,
            )?);
            phases.push(run_ai_watch_initial_index_phase(
                &syslog_bin,
                &watch_env_path,
            ));
            phases.push(systemctl_user_phase(&["daemon-reload"]));
            phases.push(systemctl_user_phase(&[
                "disable",
                "--now",
                "syslog-ai-index.timer",
            ]));
            phases.push(ai_index_timer_disabled_phase());
            phases.push(systemctl_user_phase(&[
                "reset-failed",
                "syslog-ai-watch.service",
            ]));
            phases.push(systemctl_user_required_phase(&[
                "enable",
                "--now",
                "syslog-ai-watch.service",
            ]));
        }
        AiWatchServiceAction::Remove => {
            phases.push(systemctl_user_phase(&[
                "disable",
                "--now",
                "syslog-ai-watch.service",
            ]));
            phases.push(remove_ai_watch_service_files(
                &watch_env_path,
                &service_path,
            )?);
            phases.push(systemctl_user_phase(&["daemon-reload"]));
        }
        AiWatchServiceAction::Check => {
            let syslog_bin = resolve_syslog_binary()?;
            let db_path = resolve_ai_watch_db_path(&home, &user_home)?;
            phases.push(check_file_phase(
                "ai-watch-env",
                &watch_env_path,
                "run syslog setup ai-watch-service install",
            ));
            phases.push(check_file_phase(
                "ai-watch-service",
                &service_path,
                "run syslog setup ai-watch-service install",
            ));
            phases.push(check_ai_watch_service_content_phase(
                &watch_env_path,
                &service_path,
                &state_dir,
                &syslog_bin,
                &db_path,
                &user_home,
            ));
            phases.push(ai_index_timer_disabled_phase());
            phases.push(systemctl_user_required_phase(&[
                "is-enabled",
                "syslog-ai-watch.service",
            ]));
            phases.push(systemctl_user_required_phase(&[
                "is-active",
                "syslog-ai-watch.service",
            ]));
        }
    }

    let elapsed_ms = started.elapsed().as_millis();
    let has_errors = phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error));
    Ok(SetupReport {
        mode: action.as_str(),
        elapsed_ms,
        home,
        env_path,
        compose_dir,
        data_dir,
        health_url: "host-local helper".to_string(),
        mcp_url: "host-local helper".to_string(),
        phases,
        has_errors,
    })
}

fn user_home_dir() -> io::Result<PathBuf> {
    let home =
        std::env::var("HOME").map_err(|_| io::Error::new(ErrorKind::NotFound, "HOME is unset"))?;
    Ok(PathBuf::from(home))
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

fn filesystem_phase(
    mode: SetupMode,
    home: &Path,
    data_dir: &Path,
    compose_dir: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("filesystem");
    if mode.mutates() {
        ensure_private_dir(home)?;
        ensure_private_dir(data_dir)?;
        std::fs::create_dir_all(compose_dir)?;
        return Ok(timer.finish(SetupStatus::Ok, format!("initialized {}", home.display())));
    }
    if home.is_dir() && data_dir.is_dir() && compose_dir.is_dir() {
        Ok(timer.finish(SetupStatus::Ok, format!("found {}", home.display())))
    } else {
        Ok(timer.finish(
            SetupStatus::Warn,
            format!(
                "missing setup dirs under {}; run syslog setup",
                home.display()
            ),
        ))
    }
}

fn ensure_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        std::fs::set_permissions(path, PermissionsExt::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

struct EnvResult {
    phase: SetupPhase,
    values: BTreeMap<String, String>,
}

fn ensure_env_file(path: &Path, data_dir: &Path) -> io::Result<EnvResult> {
    let timer = PhaseTimer::start("env");
    let mut env = if path.exists() {
        parse_env(&std::fs::read_to_string(path)?)
    } else {
        BTreeMap::new()
    };
    let before = env.len();

    insert_process_or_default(&mut env, "SYSLOG_HOST", "0.0.0.0");
    insert_process_or_default(&mut env, "SYSLOG_PORT", "1514");
    insert_process_or_default(&mut env, "SYSLOG_HOST_PORT", "1514");
    insert_process_or_default(&mut env, "SYSLOG_MCP_HOST", "0.0.0.0");
    insert_process_or_default(&mut env, "SYSLOG_MCP_PORT", "3100");
    insert_process_or_default(&mut env, "NO_AUTH", "false");
    insert_process_or_default(&mut env, "SYSLOG_MCP_AUTH_MODE", "bearer");
    insert_process_or_default(&mut env, "SYSLOG_MCP_DB_PATH", "/data/syslog.db");
    insert_process_or_default(
        &mut env,
        "SYSLOG_MCP_DATA_VOLUME",
        &data_dir.display().to_string(),
    );
    insert_process_or_default(&mut env, "SYSLOG_MCP_MAX_DB_SIZE_MB", "8192");
    insert_process_or_default(&mut env, "SYSLOG_MCP_RETENTION_DAYS", "90");
    insert_process_or_default(&mut env, "SYSLOG_BATCH_SIZE", "100");
    insert_process_or_default(&mut env, "SYSLOG_WRITE_CHANNEL_CAPACITY", "10000");
    insert_process_or_default(&mut env, "SYSLOG_DOCKER_INGEST_ENABLED", "false");
    insert_process_or_default(&mut env, "RUST_LOG", "info");
    insert_process_or_default(&mut env, "DOCKER_NETWORK", "syslog-mcp");
    insert_process_optional(&mut env, "SYSLOG_DOCKER_HOSTS");
    insert_process_optional(&mut env, "SYSLOG_MCP_PUBLIC_URL");
    insert_process_optional(&mut env, "SYSLOG_MCP_GOOGLE_CLIENT_ID");
    insert_process_optional(&mut env, "SYSLOG_MCP_GOOGLE_CLIENT_SECRET");
    insert_process_optional(&mut env, "SYSLOG_MCP_AUTH_ADMIN_EMAIL");
    insert_process_optional(&mut env, "SYSLOG_MCP_AUTH_ALLOWED_REDIRECT_URIS");
    insert_process_optional(&mut env, "SYSLOG_MCP_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH");

    let (uid, gid) = current_uid_gid();
    env.entry("SYSLOG_UID".to_string()).or_insert(uid);
    env.entry("SYSLOG_GID".to_string()).or_insert(gid);

    if env
        .get("NO_AUTH")
        .is_none_or(|value| !value.eq_ignore_ascii_case("true"))
        && env
            .get("SYSLOG_MCP_TOKEN")
            .is_none_or(|value| value.trim().is_empty())
    {
        env.insert("SYSLOG_MCP_TOKEN".to_string(), generate_token()?);
    }

    write_env(path, &env)?;
    let added = env.len().saturating_sub(before);
    Ok(EnvResult {
        phase: timer.finish(
            SetupStatus::Ok,
            format!("{} {} keys; added {added}", path.display(), env.len()),
        ),
        values: env,
    })
}

fn insert_process_or_default(env: &mut BTreeMap<String, String>, key: &str, default: &str) {
    if let Some(value) = process_env_value(key) {
        env.insert(key.to_string(), value);
    } else {
        env.entry(key.to_string())
            .or_insert_with(|| default.to_string());
    }
}

fn insert_process_optional(env: &mut BTreeMap<String, String>, key: &str) {
    if let Some(value) = process_env_value(key) {
        env.insert(key.to_string(), value);
    }
}

fn process_env_value(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.contains(['\n', '\r']))
}

fn parse_env(raw: &str) -> BTreeMap<String, String> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn write_env(path: &Path, env: &BTreeMap<String, String>) -> io::Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    let mut out = String::new();
    out.push_str("# syslog-mcp runtime environment.\n");
    out.push_str("# Managed by `syslog setup`; secrets are preserved on repair.\n");
    for (key, value) in env {
        out.push_str(key);
        out.push('=');
        out.push_str(value);
        out.push('\n');
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    options.open(path)?.write_all(out.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn write_compose_assets(compose_dir: &Path) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("compose-assets");
    std::fs::create_dir_all(compose_dir.join("config"))?;
    std::fs::write(
        compose_dir.join("docker-compose.yml"),
        installed_compose_asset(),
    )?;
    std::fs::write(compose_dir.join("config/Dockerfile"), DOCKERFILE_ASSET)?;
    Ok(timer.finish(
        SetupStatus::Ok,
        format!("wrote compose assets under {}", compose_dir.display()),
    ))
}

fn installed_compose_asset() -> String {
    let without_build = COMPOSE_ASSET
        .replace(
            "    # Default to the published image so plugin deploys can `docker compose pull`\n    # without source. Override with --build (or `docker compose build`) for local\n    # source development.\n    image: ghcr.io/jmagar/syslog-mcp:${SYSLOG_MCP_VERSION:-latest}\n    build:\n      context: .\n      dockerfile: config/Dockerfile\n",
            "    image: ghcr.io/jmagar/syslog-mcp:${SYSLOG_MCP_VERSION:-latest}\n",
        );
    assert_ne!(
        without_build, COMPOSE_ASSET,
        "installed compose asset transform failed: expected build stanza was not found"
    );
    let installed = without_build.replace("      - path: .env\n", "      - path: ../.env\n");
    assert_ne!(
        installed, without_build,
        "installed compose asset transform failed: expected env_file path was not found"
    );
    installed
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

fn install_ai_index_timer_files(
    bin_path: &Path,
    systemd_dir: &Path,
    service_path: &Path,
    timer_path: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("ai-index-timer-files");
    if let Some(bin_dir) = bin_path.parent() {
        std::fs::create_dir_all(bin_dir)?;
    }
    std::fs::create_dir_all(systemd_dir)?;
    write_executable_file(bin_path, &ai_index_script())?;
    std::fs::write(service_path, ai_index_service_unit(bin_path))?;
    std::fs::write(timer_path, ai_index_timer_unit())?;
    Ok(timer.finish(
        SetupStatus::Ok,
        format!(
            "wrote {}, {}, {}",
            bin_path.display(),
            service_path.display(),
            timer_path.display()
        ),
    ))
}

fn remove_ai_index_timer_files(
    bin_path: &Path,
    service_path: &Path,
    timer_path: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("ai-index-timer-files");
    for path in [bin_path, service_path, timer_path] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(timer.finish(SetupStatus::Ok, "removed syslog AI index timer files"))
}

fn install_ai_watch_service_files(
    env_path: &Path,
    service_path: &Path,
    systemd_dir: &Path,
    state_dir: &Path,
    syslog_bin: &Path,
    db_path: &Path,
    user_home: &Path,
) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("ai-watch-service-files");
    if let Some(env_dir) = env_path.parent() {
        ensure_private_dir(env_dir)?;
    }
    ensure_private_dir(state_dir)?;
    std::fs::create_dir_all(systemd_dir)?;
    write_private_file(env_path, &ai_watch_env_file(db_path))?;
    std::fs::write(
        service_path,
        ai_watch_service_unit(syslog_bin, env_path, db_path, state_dir, user_home),
    )?;
    Ok(timer.finish(
        SetupStatus::Ok,
        format!("wrote {}, {}", env_path.display(), service_path.display()),
    ))
}

fn remove_ai_watch_service_files(env_path: &Path, service_path: &Path) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("ai-watch-service-files");
    for path in [env_path, service_path] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(timer.finish(SetupStatus::Ok, "removed syslog AI watch service files"))
}

fn check_ai_watch_service_content_phase(
    env_path: &Path,
    service_path: &Path,
    state_dir: &Path,
    syslog_bin: &Path,
    db_path: &Path,
    user_home: &Path,
) -> SetupPhase {
    let timer = PhaseTimer::start("ai-watch-service-content");
    let expected_env = ai_watch_env_file(db_path);
    let expected_unit = ai_watch_service_unit(syslog_bin, env_path, db_path, state_dir, user_home);
    let current_env = match std::fs::read_to_string(env_path) {
        Ok(raw) => raw,
        Err(error) => return timer.finish(SetupStatus::Error, error.to_string()),
    };
    let current_unit = match std::fs::read_to_string(service_path) {
        Ok(raw) => raw,
        Err(error) => return timer.finish(SetupStatus::Error, error.to_string()),
    };
    if current_env != expected_env {
        return timer.finish(
            SetupStatus::Error,
            format!(
                "{} does not match generated AI watch environment",
                env_path.display()
            ),
        );
    }
    if current_unit != expected_unit {
        return timer.finish(
            SetupStatus::Error,
            format!(
                "{} does not match generated AI watch unit",
                service_path.display()
            ),
        );
    }
    timer.finish(
        SetupStatus::Ok,
        "AI watch service files match generated content",
    )
}

fn ai_watch_env_file(db_path: &Path) -> String {
    let db_path = setup_path_value(db_path).expect("validated AI watch DB path");
    format!("SYSLOG_MCP_DB_PATH={db_path}\nSYSLOG_DOCKER_INGEST_ENABLED=false\nRUST_LOG=warn\n")
}

fn ai_watch_service_unit(
    syslog_bin: &Path,
    env_path: &Path,
    db_path: &Path,
    state_dir: &Path,
    user_home: &Path,
) -> String {
    let db_dir = db_path.parent().unwrap_or_else(|| Path::new("/"));
    let env_path = setup_path_value(env_path).expect("validated AI watch env path");
    let syslog_bin = setup_path_value(syslog_bin).expect("validated syslog binary path");
    let claude_root = setup_path_value(&user_home.join(".claude/projects"))
        .expect("validated Claude transcript root");
    let codex_root = setup_path_value(&user_home.join(".codex/sessions"))
        .expect("validated Codex transcript root");
    let user_local_bin =
        setup_path_value(&user_home.join(".local/bin")).expect("validated user local bin path");
    let user_cargo_bin =
        setup_path_value(&user_home.join(".cargo/bin")).expect("validated user cargo bin path");
    let cargo_target_dir = setup_path_value(&state_dir.join("cargo-target"))
        .expect("validated AI watch cargo target directory");
    let db_dir = setup_path_value(db_dir).expect("validated AI watch DB directory");
    let state_dir = setup_path_value(state_dir).expect("validated AI watch state directory");
    format!(
        "[Unit]\nDescription=syslog-mcp real-time local AI transcript watch\nDocumentation=https://github.com/jmagar/syslog-mcp\nAfter=default.target\nStartLimitIntervalSec=300\nStartLimitBurst=5\n\n[Service]\nType=simple\nEnvironmentFile={env_path}\nEnvironment=PATH={user_local_bin}:{user_cargo_bin}:/usr/local/bin:/usr/bin:/bin\nEnvironment=CARGO_TARGET_DIR={cargo_target_dir}\nWorkingDirectory=/\nExecStart={syslog_bin} ai watch --no-initial-scan --json\nRestart=on-failure\nRestartSec=5\nUMask=0077\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nProtectHome=read-only\nBindReadOnlyPaths=-{claude_root} -{codex_root}\nBindPaths={db_dir} {state_dir}\nReadWritePaths={db_dir} {state_dir}\n\n[Install]\nWantedBy=default.target\n"
    )
}

fn run_ai_watch_initial_index_phase(syslog_bin: &Path, env_path: &Path) -> SetupPhase {
    let timer = PhaseTimer::start("ai-watch-initial-index");
    let env = parse_env(&std::fs::read_to_string(env_path).unwrap_or_default());
    let mut command = Command::new(syslog_bin);
    command.args(["ai", "index", "--json"]);
    for (key, value) in env {
        command.env(key, value);
    }
    match command.output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let status = if ai_index_output_has_failures(&stdout) {
                SetupStatus::Error
            } else {
                SetupStatus::Ok
            };
            timer.finish(status, summarize_ai_index_output(&stdout))
        }
        Ok(output) => timer.finish(
            SetupStatus::Error,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("initial AI index failed")
                .to_string(),
        ),
        Err(error) => timer.finish(SetupStatus::Error, error.to_string()),
    }
}

fn summarize_ai_index_output(stdout: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return "invalid ai index JSON output".to_string();
    };
    format!(
        "indexed files={} ingested={} duplicates={} parse_errors={} storage_blocked={} dropped_metadata_fields={} file_errors={}",
        value
            .get("discovered_files")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("ingested")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("skipped_dupes")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("parse_errors")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("storage_blocked_chunks")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("dropped_metadata_fields")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        value
            .get("file_errors")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len),
    )
}

fn ai_index_output_has_failures(stdout: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return true;
    };
    value
        .get("parse_errors")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0
        || value
            .get("storage_blocked_chunks")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        || value
            .get("dropped_metadata_fields")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        || value
            .get("file_errors")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
}

fn ai_index_timer_disabled_phase() -> SetupPhase {
    let timer = PhaseTimer::start("ai-index-timer-disabled");
    let active = systemctl_user_state("is-active", "syslog-ai-index.timer");
    let enabled = systemctl_user_state("is-enabled", "syslog-ai-index.timer");
    if active.as_deref() == Some("active") || enabled.as_deref() == Some("enabled") {
        return timer.finish(
            SetupStatus::Error,
            format!(
                "syslog-ai-index.timer still active/enabled (active={active:?}, enabled={enabled:?})"
            ),
        );
    }
    timer.finish(
        SetupStatus::Ok,
        format!(
            "syslog-ai-index.timer inactive or absent (active={active:?}, enabled={enabled:?})"
        ),
    )
}

fn systemctl_user_state(command: &str, unit: &str) -> Option<String> {
    let output = run_systemctl_user(&[command, unit]).ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stdout.is_empty()).then_some(stdout)
}

fn resolve_syslog_binary() -> io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    let output = Command::new("sh")
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(path)
}

fn db_path_from_setup_env(env_path: &Path) -> io::Result<Option<PathBuf>> {
    let raw = match std::fs::read_to_string(env_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let values = parse_env(&raw);
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

fn write_executable_file(path: &Path, body: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
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

fn write_private_file(path: &Path, body: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
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

fn ai_index_script() -> String {
    r#"#!/usr/bin/env bash
set -euo pipefail

STATE_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/syslog-mcp"
mkdir -p "$STATE_DIR"
LOCK_FILE="$STATE_DIR/ai-index.lock"
LOG_FILE="$STATE_DIR/ai-index.log"

if [[ -z "${SYSLOG_MCP_DB_PATH:-}" ]]; then
  if [[ -f "${HOME}/.claude/plugins/data/syslog-jmagar-lab/syslog.db" ]]; then
    export SYSLOG_MCP_DB_PATH="${HOME}/.claude/plugins/data/syslog-jmagar-lab/syslog.db"
  else
    export SYSLOG_MCP_DB_PATH="${SYSLOG_MCP_HOME:-${HOME}/.syslog-mcp}/data/syslog.db"
  fi
fi

export SYSLOG_DOCKER_INGEST_ENABLED="${SYSLOG_DOCKER_INGEST_ENABLED:-false}"
export RUST_LOG="${RUST_LOG:-warn}"

{
  printf '== %s ==\n' "$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
  command -v syslog
  syslog --version
  syslog ai index --json
} >>"$LOG_FILE" 2>&1
"#
    .to_string()
}

fn ai_index_service_unit(bin_path: &Path) -> String {
    format!(
        "[Unit]\nDescription=syslog-mcp local AI transcript index\nDocumentation=https://github.com/jmagar/syslog-mcp\n\n[Service]\nType=oneshot\nExecStart={}\n",
        bin_path.display()
    )
}

fn ai_index_timer_unit() -> &'static str {
    "[Unit]\nDescription=Run syslog-mcp local AI transcript index\n\n[Timer]\nOnBootSec=5min\nOnUnitActiveSec=30min\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
}

fn systemctl_user_phase(args: &[&str]) -> SetupPhase {
    let timer = PhaseTimer::start("systemctl-user");
    match run_systemctl_user(args) {
        Ok(output) if output.status.success() => timer.finish(
            SetupStatus::Ok,
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("ok")
                .to_string(),
        ),
        Ok(output) => timer.finish(
            SetupStatus::Warn,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("systemctl --user failed")
                .to_string(),
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            timer.finish(SetupStatus::Warn, "systemctl not found")
        }
        Err(error) => timer.finish(SetupStatus::Warn, error.to_string()),
    }
}

fn systemctl_user_required_phase(args: &[&str]) -> SetupPhase {
    let timer = PhaseTimer::start("systemctl-user");
    match run_systemctl_user(args) {
        Ok(output) if output.status.success() => timer.finish(
            SetupStatus::Ok,
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("ok")
                .to_string(),
        ),
        Ok(output) => timer.finish(
            SetupStatus::Error,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("systemctl --user failed")
                .to_string(),
        ),
        Err(error) => timer.finish(SetupStatus::Error, error.to_string()),
    }
}

fn run_systemctl_user(args: &[&str]) -> io::Result<std::process::Output> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()?;
    if output.status.success() || std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.contains("DBUS_SESSION_BUS_ADDRESS") && !stderr.contains("user scope bus") {
        return Ok(output);
    }
    let Some((runtime_dir, bus_address)) = inferred_user_bus_env() else {
        return Ok(output);
    };
    Command::new("systemctl")
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("DBUS_SESSION_BUS_ADDRESS", bus_address)
        .arg("--user")
        .args(args)
        .output()
}

fn inferred_user_bus_env() -> Option<(PathBuf, String)> {
    let uid = current_uid_gid().0;
    let runtime_dir = PathBuf::from(format!("/run/user/{uid}"));
    let bus = runtime_dir.join("bus");
    bus.exists()
        .then(|| (runtime_dir, format!("unix:path={}", bus.display())))
}

fn command_phase<const N: usize>(name: &'static str, args: [&str; N]) -> SetupPhase {
    let timer = PhaseTimer::start(name);
    let program = if name == "docker compose" {
        "docker"
    } else {
        name
    };
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => timer.finish(
            SetupStatus::Ok,
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("available")
                .to_string(),
        ),
        Ok(output) => timer.finish(
            SetupStatus::Error,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("command failed")
                .to_string(),
        ),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            timer.finish(SetupStatus::Error, "not found on PATH")
        }
        Err(err) => timer.finish(SetupStatus::Error, err.to_string()),
    }
}

fn cleanup_legacy_systemd() -> SetupPhase {
    let timer = PhaseTimer::start("legacy-systemd");
    let home = match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home),
        Err(_) => {
            return timer.finish(SetupStatus::Skipped, "HOME unset");
        }
    };
    let mut failures = Vec::new();
    for unit in [
        "syslog-mcp.service",
        "mnemo-index.service",
        "mnemo-index.timer",
    ] {
        match Command::new("systemctl")
            .args(["--user", "disable", "--now", unit])
            .output()
        {
            Ok(output) if output.status.success() => {}
            Ok(output) => failures.push(format!(
                "systemctl disable --now {unit}: {}",
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("failed")
            )),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => failures.push(format!("systemctl disable --now {unit}: {error}")),
        }
    }
    for name in [
        "syslog-mcp.service",
        "mnemo-index.service",
        "mnemo-index.timer",
    ] {
        let unit = home.join(".config/systemd/user").join(name);
        let dropins = home.join(".config/systemd/user").join(format!("{name}.d"));
        if let Err(error) = std::fs::remove_file(&unit) {
            if error.kind() != ErrorKind::NotFound {
                failures.push(format!("remove {}: {error}", unit.display()));
            }
        }
        if let Err(error) = std::fs::remove_dir_all(&dropins) {
            if error.kind() != ErrorKind::NotFound {
                failures.push(format!("remove {}: {error}", dropins.display()));
            }
        }
    }
    if let Err(error) = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output()
    {
        if error.kind() != ErrorKind::NotFound {
            failures.push(format!("systemctl daemon-reload: {error}"));
        }
    }
    if !failures.is_empty() {
        return timer.finish(SetupStatus::Warn, failures.join("; "));
    }
    timer.finish(
        SetupStatus::Ok,
        "removed stale syslog-mcp and mnemo-index user units/drop-ins if present",
    )
}

fn ensure_network_phase(phases: &mut Vec<SetupPhase>, env: Option<&BTreeMap<String, String>>) {
    let timer = PhaseTimer::start("docker-network");
    let network = env
        .and_then(|env| env.get("DOCKER_NETWORK"))
        .map(String::as_str)
        .unwrap_or("syslog-mcp");
    let inspect = Command::new("docker")
        .args(["network", "inspect", network])
        .output();
    if inspect.as_ref().is_ok_and(|output| output.status.success()) {
        phases.push(timer.finish(SetupStatus::Ok, format!("{network} exists")));
        return;
    }
    match Command::new("docker")
        .args(["network", "create", network])
        .output()
    {
        Ok(output) if output.status.success() => {
            phases.push(timer.finish(SetupStatus::Ok, format!("created {network}")))
        }
        Ok(output) => phases.push(
            timer.finish(
                SetupStatus::Error,
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("docker network create failed"),
            ),
        ),
        Err(err) => phases.push(timer.finish(SetupStatus::Error, err.to_string())),
    }
}

fn run_compose_phase(compose_dir: &Path, env_path: &Path, args: &[&str]) -> SetupPhase {
    let timer = PhaseTimer::start(if args.first() == Some(&"pull") {
        "compose-pull"
    } else {
        "compose-up"
    });
    let mut command = Command::new("docker");
    command
        .arg("compose")
        .arg("--env-file")
        .arg(env_path)
        .arg("-f")
        .arg(compose_dir.join("docker-compose.yml"));
    let override_path = compose_dir.join("docker-compose.override.yml");
    if override_path.exists() {
        command.arg("-f").arg(override_path);
    }
    command.args(args).current_dir(compose_dir);
    match command.output() {
        Ok(output) if output.status.success() => timer.finish(
            SetupStatus::Ok,
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .last()
                .unwrap_or("ok")
                .to_string(),
        ),
        Ok(output) => timer.finish(
            SetupStatus::Error,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .last()
                .unwrap_or("docker compose failed")
                .to_string(),
        ),
        Err(err) => timer.finish(SetupStatus::Error, err.to_string()),
    }
}

fn health_phase(env: &Option<BTreeMap<String, String>>) -> SetupPhase {
    let timer = PhaseTimer::start("health");
    let port = env
        .as_ref()
        .and_then(|env| env.get("SYSLOG_MCP_PORT"))
        .map(String::as_str)
        .unwrap_or("3100");
    let url = format!("http://127.0.0.1:{port}/health");
    match Command::new("curl")
        .args(["-fsS", "--max-time", "5", &url])
        .output()
    {
        Ok(output) if output.status.success() => {
            timer.finish(SetupStatus::Ok, format!("{url} ready"))
        }
        Ok(output) => timer.finish(
            SetupStatus::Error,
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("health check failed"),
        ),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            timer.finish(SetupStatus::Error, "curl not found; skipped health check")
        }
        Err(err) => timer.finish(SetupStatus::Error, err.to_string()),
    }
}

fn current_uid_gid() -> (String, String) {
    let uid = command_stdout("id", ["-u"]).unwrap_or_else(|| "1000".to_string());
    let gid = command_stdout("id", ["-g"]).unwrap_or_else(|| "1000".to_string());
    (uid, gid)
}

fn command_stdout<const N: usize>(program: &str, args: [&str; N]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn generate_token() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    let mut token = String::with_capacity(64);
    for byte in bytes {
        token.push_str(&format!("{byte:02x}"));
    }
    Ok(token)
}
