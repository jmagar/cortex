use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde::Serialize;

const DIAG_DOCKER_UNAVAILABLE: &str = "docker_unavailable";
const DIAG_TARGET_UNRESOLVED: &str = "target_unresolved";
const DIAG_SYSTEMD_CHECK_FAILED: &str = "systemd_check_failed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeDefaults {
    pub service: String,
    pub container_name: String,
    pub timeout: Duration,
    pub output_limit_bytes: usize,
}

impl Default for ComposeDefaults {
    fn default() -> Self {
        Self {
            service: "syslog-mcp".into(),
            container_name: "syslog-mcp".into(),
            timeout: Duration::from_secs(120),
            output_limit_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposeTarget {
    pub project_dir: Option<PathBuf>,
    pub compose_file: Option<PathBuf>,
    pub project_name: Option<String>,
    pub service: Option<String>,
    pub container_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSource {
    Explicit,
    LiveContainerLabels,
    CurrentWorkingDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetConfidence {
    Confirmed,
    Ambiguous,
    Unsafe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Unsafe,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComposeDiagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedComposeTarget {
    pub target: ComposeTargetSummary,
    pub source: TargetSource,
    pub confidence: TargetConfidence,
    pub diagnostics: Vec<ComposeDiagnostic>,
    pub compose_files: Vec<PathBuf>,
    pub compose_working_dir: Option<PathBuf>,
    pub compose_project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComposeTargetSummary {
    pub project_dir: Option<PathBuf>,
    pub compose_file: Option<PathBuf>,
    pub project_name: Option<String>,
    pub service: String,
    pub container_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MountInfo {
    pub source: Option<PathBuf>,
    pub target: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortInfo {
    pub private_port: u16,
    pub public_port: Option<u16>,
    pub protocol: String,
    pub host_ip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdStatus {
    pub unit: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComposeStatus {
    pub container_name: String,
    pub container_id: Option<String>,
    pub status: Option<String>,
    pub health: Option<String>,
    pub image: Option<String>,
    pub image_id: Option<String>,
    pub compose_project: Option<String>,
    pub compose_working_dir: Option<PathBuf>,
    pub compose_files: Vec<PathBuf>,
    pub service: Option<String>,
    pub data_mounts: Vec<MountInfo>,
    pub ports: Vec<PortInfo>,
    pub systemd: Option<SystemdStatus>,
    pub diagnostics: Vec<ComposeDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicPortSummary {
    pub port: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeOwnershipState {
    ComposeOwned,
    OwnerMismatch,
    SystemdOwned,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeRuntimeState {
    Healthy,
    Degraded,
    Stopped,
    DockerUnavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComposeMcpDiagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComposeMcpStatus {
    pub container_name: String,
    pub ownership: ComposeOwnershipState,
    pub runtime_state: ComposeRuntimeState,
    pub health: Option<String>,
    pub published_ports: Vec<PublicPortSummary>,
    pub diagnostics: Vec<ComposeMcpDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComposeDryRun {
    pub dry_run: bool,
    pub command: Vec<String>,
    pub target: ComposeTargetSummary,
    pub preflight: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComposeCommandResult {
    Executed(CommandOutput),
    DryRun(ComposeDryRun),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub status: Option<String>,
    pub health: Option<String>,
    pub image: Option<String>,
    pub image_id: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub mounts: Vec<MountInfo>,
    pub ports: Vec<PortInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerInfo {
    pub port: u16,
    pub process: Option<String>,
    pub belongs_to_target: bool,
}

pub trait DockerInspect {
    fn inspect_container(&self, name: &str) -> Result<Option<ContainerInfo>>;
    fn find_candidates(&self, service: &str, container_name: &str) -> Result<Vec<ContainerInfo>>;
    fn systemd_status(&self, unit: &str) -> Result<Option<SystemdStatus>>;
    fn listeners(&self, ports: &[u16]) -> Result<Vec<ListenerInfo>>;
    fn published_port_owner(&self, _port: u16) -> Result<Option<String>> {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeMutation {
    Up,
    Down,
    Restart,
    Pull,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutationOptions {
    pub dry_run: bool,
    pub allow_cwd_target: bool,
    pub yes: bool,
    pub non_interactive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub current_dir: Option<PathBuf>,
    pub timeout: Duration,
    pub output_limit_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TimeoutCleanupStatus {
    pub terminate_sent: bool,
    pub kill_sent: bool,
    pub reaped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandOutput {
    pub exit_status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
    pub timeout_cleanup: Option<TimeoutCleanupStatus>,
}

pub trait CommandRunner {
    fn run(&self, invocation: &ComposeInvocation) -> Result<CommandOutput>;
}

pub struct ComposeService<I, R> {
    inspector: I,
    runner: R,
    defaults: ComposeDefaults,
}

impl<I, R> ComposeService<I, R> {
    pub fn new(inspector: I, runner: R, defaults: ComposeDefaults) -> Self {
        Self {
            inspector,
            runner,
            defaults,
        }
    }

    fn compose_invocation(
        &self,
        target: &ResolvedComposeTarget,
        mutation: ComposeMutation,
    ) -> ComposeInvocation {
        let mut args = compose_base_args(target);
        args.extend(compose_mutation_args(mutation, &target.target.service));
        self.invocation(target, args)
    }

    fn logs_invocation(&self, target: &ResolvedComposeTarget, tail: u32) -> ComposeInvocation {
        let mut args = compose_base_args(target);
        args.push("logs".into());
        args.push("--tail".into());
        args.push(tail.to_string());
        args.push(target.target.service.clone());
        self.invocation(target, args)
    }

    fn invocation(&self, target: &ResolvedComposeTarget, args: Vec<String>) -> ComposeInvocation {
        ComposeInvocation {
            program: "docker".into(),
            args,
            current_dir: target.compose_working_dir.clone(),
            timeout: self.defaults.timeout,
            output_limit_bytes: self.defaults.output_limit_bytes,
        }
    }
}

fn compose_base_args(target: &ResolvedComposeTarget) -> Vec<String> {
    let mut args = vec!["compose".into()];
    if let Some(project_dir) = &target.compose_working_dir {
        args.push("--project-directory".into());
        args.push(project_dir.display().to_string());
        // Docker Compose only looks for .env in the project directory for YAML
        // variable substitution. The syslog-mcp setup places .env one level up
        // (e.g. ~/.syslog-mcp/.env with compose files under ~/.syslog-mcp/compose/).
        // Pass --env-file explicitly so SYSLOG_MCP_DATA_VOLUME and similar vars
        // are substituted correctly (bind-mount vs named-volume fallback).
        if let Some(env_path) = project_dir
            .parent()
            .map(|p| p.join(".env"))
            .filter(|p| p.is_file())
        {
            args.push("--env-file".into());
            args.push(env_path.display().to_string());
        }
    }
    for file in &target.compose_files {
        args.push("-f".into());
        args.push(file.display().to_string());
    }
    if let Some(project_name) = &target.compose_project {
        args.push("--project-name".into());
        args.push(project_name.clone());
    }
    args
}

fn compose_mutation_args(mutation: ComposeMutation, service: &str) -> Vec<String> {
    let mut args = Vec::new();
    match mutation {
        ComposeMutation::Up => {
            args.push("up".into());
            args.push("-d".into());
        }
        ComposeMutation::Restart => {
            args.push("restart".into());
        }
        ComposeMutation::Pull => {
            args.push("pull".into());
        }
        ComposeMutation::Down => {
            args.push("stop".into());
        }
    }
    args.push(service.into());
    args
}

impl<I: DockerInspect, R> ComposeService<I, R> {
    pub fn resolve_target(&self, requested: &ComposeTarget) -> Result<ResolvedComposeTarget> {
        let service = requested
            .service
            .clone()
            .unwrap_or_else(|| self.defaults.service.clone());
        let container_name = requested
            .container_name
            .clone()
            .unwrap_or_else(|| self.defaults.container_name.clone());

        if requested.compose_file.is_some() || requested.project_dir.is_some() {
            return Ok(ResolvedComposeTarget {
                target: ComposeTargetSummary {
                    project_dir: requested.project_dir.clone(),
                    compose_file: requested.compose_file.clone(),
                    project_name: requested.project_name.clone(),
                    service,
                    container_name,
                },
                source: TargetSource::Explicit,
                confidence: TargetConfidence::Confirmed,
                diagnostics: Vec::new(),
                compose_files: requested.compose_file.clone().into_iter().collect(),
                compose_working_dir: requested.project_dir.clone(),
                compose_project: requested.project_name.clone(),
            });
        }

        if let Some(info) = self.inspector.inspect_container(&container_name)? {
            let target = target_from_container(&info, &self.defaults);
            validate_requested_selectors(requested, &target)?;
            return Ok(target);
        }

        let candidates = self.inspector.find_candidates(&service, &container_name)?;
        if candidates.len() == 1 {
            let target = target_from_container(&candidates[0], &self.defaults);
            validate_requested_selectors(requested, &target)?;
            return Ok(target);
        }
        if candidates.len() > 1 {
            return Ok(ResolvedComposeTarget {
                target: ComposeTargetSummary {
                    project_dir: None,
                    compose_file: None,
                    project_name: requested.project_name.clone(),
                    service,
                    container_name,
                },
                source: TargetSource::LiveContainerLabels,
                confidence: TargetConfidence::Ambiguous,
                diagnostics: vec![ComposeDiagnostic {
                    severity: DiagnosticSeverity::Unsafe,
                    code: "multiple_compose_candidates".into(),
                    message: format!("found {} candidate syslog-mcp containers", candidates.len()),
                }],
                compose_files: Vec::new(),
                compose_working_dir: None,
                compose_project: requested.project_name.clone(),
            });
        }

        let cwd = std::env::current_dir()?;
        let cwd_file = cwd.join("docker-compose.yml");
        if cwd_file.exists() {
            return Ok(ResolvedComposeTarget {
                target: ComposeTargetSummary {
                    project_dir: Some(cwd.clone()),
                    compose_file: Some(cwd_file.clone()),
                    project_name: requested.project_name.clone(),
                    service,
                    container_name,
                },
                source: TargetSource::CurrentWorkingDirectory,
                confidence: TargetConfidence::Unsafe,
                diagnostics: vec![ComposeDiagnostic {
                    severity: DiagnosticSeverity::Unsafe,
                    code: "cwd_fallback_requires_confirmation".into(),
                    message:
                        "cwd docker-compose.yml is not enough for mutation without --allow-cwd-target"
                            .into(),
                }],
                compose_files: vec![cwd_file],
                compose_working_dir: Some(cwd),
                compose_project: requested.project_name.clone(),
            });
        }

        Err(anyhow!("could not resolve syslog-mcp compose target"))
    }

    pub fn preflight_mutation(
        &self,
        mutation: ComposeMutation,
        target: &ResolvedComposeTarget,
        options: &MutationOptions,
    ) -> Result<()> {
        if target.confidence == TargetConfidence::Ambiguous {
            return Err(anyhow!("refusing mutation: target is ambiguous"));
        }
        if target.source == TargetSource::CurrentWorkingDirectory && !options.allow_cwd_target {
            return Err(anyhow!(
                "refusing mutation: cwd target requires --allow-cwd-target"
            ));
        }
        if target.target.project_name.is_some()
            && target.target.project_dir.is_none()
            && target.target.compose_file.is_none()
            && target.source != TargetSource::LiveContainerLabels
        {
            return Err(anyhow!(
                "refusing mutation: --project-name alone is not a safe target"
            ));
        }
        for file in &target.compose_files {
            if !file.exists() {
                return Err(anyhow!(
                    "refusing mutation: compose file does not exist: {}",
                    file.display()
                ));
            }
        }

        if target.source == TargetSource::LiveContainerLabels
            && target.confidence != TargetConfidence::Confirmed
        {
            return Err(anyhow!(
                "refusing mutation: live container is missing required compose labels"
            ));
        }

        let ownership = if mutation_requires_ownership_probe(mutation) {
            let target_container = self
                .inspector
                .inspect_container(&target.target.container_name)?;
            let target_container_id = target_container.as_ref().map(|info| info.id.as_str());
            let published_ports = target_container
                .as_ref()
                .map(|info| {
                    info.ports
                        .iter()
                        .filter_map(|port| port.public_port)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let systemd = self
                .inspector
                .systemd_status("syslog-mcp.service")
                .map_err(|error| {
                    anyhow!("refusing mutation: could not verify systemd ownership: {error}")
                })?;
            let listeners = self.inspector.listeners(&[1514, 3100]).map_err(|error| {
                anyhow!("refusing mutation: could not verify port listeners: {error}")
            })?;
            let mut non_target_listener = false;
            for listener in &listeners {
                if !listener_belongs_to_target(
                    &self.inspector,
                    listener,
                    &target.target.container_name,
                    target_container_id,
                    &published_ports,
                )? {
                    non_target_listener = true;
                    break;
                }
            }
            Some((
                systemd.as_ref().is_some_and(|s| s.active),
                non_target_listener,
            ))
        } else {
            None
        };

        match mutation {
            ComposeMutation::Up | ComposeMutation::Restart
                if ownership.is_some_and(|(systemd_active, non_target_listener)| {
                    systemd_active || non_target_listener
                }) =>
            {
                return Err(anyhow!(
                    "refusing mutation: systemd or non-target listener owns syslog ports"
                ));
            }
            ComposeMutation::Down if target.source != TargetSource::LiveContainerLabels => {
                return Err(anyhow!(
                    "refusing down: target must be confirmed compose-owned"
                ));
            }
            ComposeMutation::Down if options.non_interactive && !options.yes => {
                return Err(anyhow!(
                    "refusing down: --yes is required in non-interactive mode"
                ));
            }
            ComposeMutation::Pull
            | ComposeMutation::Up
            | ComposeMutation::Restart
            | ComposeMutation::Down => {}
        }

        Ok(())
    }

    pub fn status(&self, requested: &ComposeTarget) -> Result<ComposeStatus> {
        let target = match self.resolve_target(requested) {
            Ok(target) => target,
            Err(error) => {
                return Ok(unresolved_status(
                    requested,
                    &self.defaults,
                    unresolved_code(&error),
                    format!("could not resolve syslog-mcp compose target: {error}"),
                ));
            }
        };
        let container_name = target.target.container_name.clone();
        let info = match self.inspector.inspect_container(&container_name) {
            Ok(info) => info,
            Err(error) => {
                let mut status = status_from_target(target, None, None);
                status.diagnostics.push(ComposeDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    code: unresolved_code(&error).into(),
                    message: error.to_string(),
                });
                return Ok(status);
            }
        };
        let mut systemd_error = None;
        let systemd = match self.inspector.systemd_status("syslog-mcp.service") {
            Ok(systemd) => systemd,
            Err(error) => {
                systemd_error = Some(error.to_string());
                None
            }
        };
        let mut status = status_from_target(target, info, systemd);
        // Detect DB drift: the container's /data must be a bind mount, not a
        // Docker named volume. A named volume means --env-file wasn't passed to
        // compose (SYSLOG_MCP_DATA_VOLUME substitution failed), so the container
        // writes to an isolated volume while the CLI reads from a different file.
        // Only check when the container is in a running state (mounts are populated
        // for stopped containers too but the status field indicates it's running).
        if status.status.as_deref().map(|s| !is_stopped_status(s)).unwrap_or(false) {
            let data_mount = status.data_mounts.iter().find(|m| m.target == "/data");
            match data_mount {
                None => {
                    status.diagnostics.push(ComposeDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        code: "data_volume_missing".into(),
                        message: "container has no /data mount — syslog DB is inaccessible"
                            .into(),
                    });
                }
                Some(mount) if mount.kind != "bind" => {
                    status.diagnostics.push(ComposeDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        code: "data_volume_not_bind".into(),
                        message: format!(
                            "/data is a {} (not a bind mount) — container and CLI are \
                             using separate databases; run `syslog compose up` to recreate",
                            mount.kind
                        ),
                    });
                }
                Some(_) => {}
            }
        }
        if let Some(error) = systemd_error {
            status.diagnostics.push(ComposeDiagnostic {
                severity: DiagnosticSeverity::Error,
                code: DIAG_SYSTEMD_CHECK_FAILED.into(),
                message: format!("could not verify syslog-mcp.service state: {error}"),
            });
        }
        if status.systemd.as_ref().is_some_and(|s| s.active) {
            let code = if status.compose_project.is_some() {
                "owner_mismatch"
            } else {
                "systemd_active"
            };
            status.diagnostics.push(ComposeDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: code.into(),
                message: "syslog-mcp.service is active".into(),
            });
        }
        Ok(status)
    }
}

impl<I: DockerInspect, R: CommandRunner> ComposeService<I, R> {
    pub fn run_mutation(
        &self,
        mutation: ComposeMutation,
        requested: &ComposeTarget,
        options: &MutationOptions,
    ) -> Result<ComposeCommandResult> {
        let target = self.resolve_target(requested)?;
        self.preflight_mutation(mutation, &target, options)?;
        let invocation = self.compose_invocation(&target, mutation);
        if options.dry_run {
            return Ok(ComposeCommandResult::DryRun(ComposeDryRun {
                dry_run: true,
                command: std::iter::once(invocation.program.clone())
                    .chain(invocation.args.clone())
                    .collect(),
                target: target.target,
                preflight: "passed".into(),
            }));
        }
        self.runner
            .run(&invocation)
            .map(ComposeCommandResult::Executed)
    }

    pub fn logs(&self, requested: &ComposeTarget, tail: Option<u32>) -> Result<CommandOutput> {
        let target = self.resolve_target(requested)?;
        if target.source == TargetSource::CurrentWorkingDirectory {
            return Err(anyhow!(
                "refusing logs: cwd target requires explicit compose target"
            ));
        }
        if target.confidence != TargetConfidence::Confirmed {
            return Err(anyhow!("refusing logs: target is not confirmed"));
        }
        let invocation = self.logs_invocation(&target, tail.unwrap_or(100));
        self.runner.run(&invocation)
    }
}

fn listener_belongs_to_target<I: DockerInspect>(
    inspector: &I,
    listener: &ListenerInfo,
    container_name: &str,
    container_id: Option<&str>,
    published_ports: &[u16],
) -> Result<bool> {
    if listener.belongs_to_target {
        return Ok(true);
    }
    if !published_ports.contains(&listener.port) {
        return Ok(false);
    }
    // If ss shows process info with the users: field, verify it is docker-proxy.
    // Any other named process definitively owns the port and is not our target.
    // When running without root, ss omits process info (no users: field), so we
    // fall through to the docker ps ownership check below.
    if let Some(process) = listener.process.as_deref() {
        if process.contains("users:") && !process.contains("docker-proxy") {
            return Ok(false);
        }
    }
    // Use docker ps --filter publish=PORT to confirm ownership. This works without
    // root privileges, unlike ss process inspection, and handles the common
    // non-root deployment scenario correctly.
    let Some(owner) = inspector.published_port_owner(listener.port)? else {
        return Ok(false);
    };
    Ok(owner == container_name
        || container_id.is_some_and(|id| id == owner || id.starts_with(&owner)))
}

fn unresolved_status(
    requested: &ComposeTarget,
    defaults: &ComposeDefaults,
    code: &str,
    message: String,
) -> ComposeStatus {
    ComposeStatus {
        container_name: requested
            .container_name
            .clone()
            .unwrap_or_else(|| defaults.container_name.clone()),
        container_id: None,
        status: None,
        health: None,
        image: None,
        image_id: None,
        compose_project: requested.project_name.clone(),
        compose_working_dir: requested.project_dir.clone(),
        compose_files: requested.compose_file.clone().into_iter().collect(),
        service: Some(
            requested
                .service
                .clone()
                .unwrap_or_else(|| defaults.service.clone()),
        ),
        data_mounts: Vec::new(),
        ports: Vec::new(),
        systemd: None,
        diagnostics: vec![ComposeDiagnostic {
            severity: DiagnosticSeverity::Error,
            code: code.into(),
            message,
        }],
    }
}

fn unresolved_code(error: &anyhow::Error) -> &'static str {
    if error.downcast_ref::<DockerUnavailableError>().is_some() {
        DIAG_DOCKER_UNAVAILABLE
    } else {
        DIAG_TARGET_UNRESOLVED
    }
}

#[derive(Debug)]
struct DockerUnavailableError(String);

impl fmt::Display for DockerUnavailableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "docker unavailable: {}", self.0)
    }
}

impl Error for DockerUnavailableError {}

fn status_from_target(
    target: ResolvedComposeTarget,
    info: Option<ContainerInfo>,
    systemd: Option<SystemdStatus>,
) -> ComposeStatus {
    let mut diagnostics = target.diagnostics.clone();
    if target.source == TargetSource::CurrentWorkingDirectory {
        diagnostics.push(ComposeDiagnostic {
            severity: DiagnosticSeverity::Warning,
            code: "cwd_target".into(),
            message: "resolved from current working directory".into(),
        });
    }
    ComposeStatus {
        container_name: target.target.container_name,
        container_id: info.as_ref().map(|i| i.id.clone()),
        status: info.as_ref().and_then(|i| i.status.clone()),
        health: info.as_ref().and_then(|i| i.health.clone()),
        image: info.as_ref().and_then(|i| i.image.clone()),
        image_id: info.as_ref().and_then(|i| i.image_id.clone()),
        compose_project: target.compose_project,
        compose_working_dir: target.compose_working_dir,
        compose_files: target.compose_files,
        service: Some(target.target.service),
        data_mounts: info.as_ref().map(|i| i.mounts.clone()).unwrap_or_default(),
        ports: info.as_ref().map(|i| i.ports.clone()).unwrap_or_default(),
        systemd,
        diagnostics,
    }
}

fn label<'a>(info: &'a ContainerInfo, key: &str) -> Option<&'a str> {
    info.labels.get(key).map(String::as_str)
}

fn split_compose_files(value: Option<&str>) -> Vec<PathBuf> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn target_from_container(
    info: &ContainerInfo,
    defaults: &ComposeDefaults,
) -> ResolvedComposeTarget {
    let project = label(info, "com.docker.compose.project").map(str::to_string);
    let service_label = label(info, "com.docker.compose.service").map(str::to_string);
    let working_dir_label =
        label(info, "com.docker.compose.project.working_dir").map(PathBuf::from);
    let config_files_label = label(info, "com.docker.compose.project.config_files");
    let compose_files = split_compose_files(config_files_label);

    let mut missing = Vec::new();
    if project.is_none() {
        missing.push("com.docker.compose.project");
    }
    if service_label.is_none() {
        missing.push("com.docker.compose.service");
    }
    if working_dir_label.is_none() {
        missing.push("com.docker.compose.project.working_dir");
    }
    if compose_files.is_empty() {
        missing.push("com.docker.compose.project.config_files");
    }

    let service = service_label.unwrap_or_else(|| defaults.service.clone());
    let container_name = info.name.trim_start_matches('/').to_string();
    let diagnostics = if missing.is_empty() {
        Vec::new()
    } else {
        vec![ComposeDiagnostic {
            severity: DiagnosticSeverity::Unsafe,
            code: "incomplete_compose_labels".into(),
            message: format!(
                "container is missing required compose labels: {}",
                missing.join(", ")
            ),
        }]
    };
    let confidence = if missing.is_empty() {
        TargetConfidence::Confirmed
    } else {
        TargetConfidence::Unsafe
    };
    ResolvedComposeTarget {
        target: ComposeTargetSummary {
            project_dir: working_dir_label.clone(),
            compose_file: compose_files.first().cloned(),
            project_name: project.clone(),
            service,
            container_name,
        },
        source: TargetSource::LiveContainerLabels,
        confidence,
        diagnostics,
        compose_files,
        compose_working_dir: working_dir_label,
        compose_project: project,
    }
}

fn mutation_requires_ownership_probe(mutation: ComposeMutation) -> bool {
    matches!(
        mutation,
        ComposeMutation::Up | ComposeMutation::Restart | ComposeMutation::Down
    )
}

fn validate_requested_selectors(
    requested: &ComposeTarget,
    target: &ResolvedComposeTarget,
) -> Result<()> {
    if let Some(project_name) = &requested.project_name {
        if target.target.project_name.as_ref() != Some(project_name) {
            return Err(anyhow!(
                "requested project_name {project_name:?} does not match resolved compose project {:?}",
                target.target.project_name
            ));
        }
    }
    if let Some(service) = &requested.service {
        if &target.target.service != service {
            return Err(anyhow!(
                "requested service {service:?} does not match resolved compose service {:?}",
                target.target.service
            ));
        }
    }
    Ok(())
}

pub fn redact_sensitive(input: &str) -> String {
    let sensitive = [
        "token",
        "secret",
        "key",
        "password",
        "client_secret",
        "authorization",
    ];
    input
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if sensitive.iter().any(|term| lower.contains(term)) {
                "[REDACTED]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn mcp_projection(status: &ComposeStatus) -> ComposeMcpStatus {
    let has_hard_diagnostic = status.diagnostics.iter().any(|d| {
        matches!(
            d.severity,
            DiagnosticSeverity::Error | DiagnosticSeverity::Unsafe
        )
    });
    let ownership = if status
        .diagnostics
        .iter()
        .any(|d| d.code == "owner_mismatch")
    {
        ComposeOwnershipState::OwnerMismatch
    } else if status.systemd.as_ref().is_some_and(|s| s.active) {
        ComposeOwnershipState::SystemdOwned
    } else if has_hard_diagnostic {
        ComposeOwnershipState::Unknown
    } else if status.compose_project.is_some() {
        ComposeOwnershipState::ComposeOwned
    } else {
        ComposeOwnershipState::Unknown
    };

    let runtime_state = if status
        .diagnostics
        .iter()
        .any(|d| d.code == DIAG_DOCKER_UNAVAILABLE)
    {
        ComposeRuntimeState::DockerUnavailable
    } else if has_hard_diagnostic {
        ComposeRuntimeState::Degraded
    } else {
        match status.health.as_deref() {
            Some("healthy") => ComposeRuntimeState::Healthy,
            Some("unhealthy") => ComposeRuntimeState::Degraded,
            _ if status.status.as_deref().is_some_and(is_stopped_status) => {
                ComposeRuntimeState::Stopped
            }
            _ => ComposeRuntimeState::Unknown,
        }
    };

    ComposeMcpStatus {
        container_name: status.container_name.clone(),
        ownership,
        runtime_state,
        health: status.health.clone(),
        published_ports: status
            .ports
            .iter()
            .filter_map(|port| {
                port.public_port.map(|public| PublicPortSummary {
                    port: public,
                    protocol: port.protocol.clone(),
                })
            })
            .collect(),
        diagnostics: status
            .diagnostics
            .iter()
            .map(|d| ComposeMcpDiagnostic {
                severity: d.severity.clone(),
                code: d.code.clone(),
            })
            .collect(),
    }
}

pub fn ensure_doctor_ready(status: &ComposeStatus) -> Result<()> {
    let projected = mcp_projection(status);
    if projected.ownership != ComposeOwnershipState::ComposeOwned
        || projected.runtime_state != ComposeRuntimeState::Healthy
        || status.diagnostics.iter().any(|d| {
            matches!(
                d.severity,
                DiagnosticSeverity::Error | DiagnosticSeverity::Unsafe
            )
        })
    {
        return Err(anyhow!(
            "compose doctor failed: ownership={:?} runtime_state={:?} diagnostics={:?}",
            projected.ownership,
            projected.runtime_state,
            status.diagnostics
        ));
    }
    Ok(())
}

fn is_stopped_status(status: &str) -> bool {
    let status = status.to_ascii_lowercase();
    status.contains("exited") || status == "stopped"
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CliDockerInspect;

impl DockerInspect for CliDockerInspect {
    fn inspect_container(&self, name: &str) -> Result<Option<ContainerInfo>> {
        let output = run_inspector_command(
            "docker",
            &["inspect", name, "--format", "{{json .}}"],
            Duration::from_secs(10),
        )
        .map_err(|e| DockerUnavailableError(format!("docker inspect failed: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
            if !stderr.contains("no such object") && !stderr.contains("no such container") {
                return Err(DockerUnavailableError(format!(
                    "docker inspect failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
                .into());
            }
            return Ok(None);
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        container_info_from_inspect(value).map(Some)
    }

    fn find_candidates(&self, service: &str, container_name: &str) -> Result<Vec<ContainerInfo>> {
        let filter = format!("label=com.docker.compose.service={service}");
        let output = run_inspector_command(
            "docker",
            &["ps", "-a", "--filter", &filter, "--format", "{{.Names}}"],
            Duration::from_secs(10),
        )
        .map_err(|e| DockerUnavailableError(format!("docker ps failed: {e}")))?;
        if !output.status.success() {
            return Err(DockerUnavailableError(format!(
                "docker ps failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
            .into());
        }
        let names = String::from_utf8_lossy(&output.stdout);
        let mut found = Vec::new();
        for name in names.lines().take(10) {
            if name == container_name || name.contains(service) {
                if let Some(info) = self.inspect_container(name)? {
                    found.push(info);
                }
            }
        }
        Ok(found)
    }

    fn systemd_status(&self, unit: &str) -> Result<Option<SystemdStatus>> {
        let output = run_inspector_command(
            "systemctl",
            &["--user", "is-active", unit],
            Duration::from_secs(3),
        )?;
        systemd_status_from_output(unit, &output)
    }

    fn listeners(&self, ports: &[u16]) -> Result<Vec<ListenerInfo>> {
        let mut listeners = Vec::new();
        for port in ports {
            let port_arg = format!(":{port}");
            let output = run_inspector_command(
                "ss",
                &["-H", "-ltnup", "sport", "=", &port_arg],
                Duration::from_secs(3),
            )?;
            if !output.status.success() {
                return Err(anyhow!(
                    "ss listener check failed for port {port}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            if ss_output_has_listener(&output.stdout) {
                listeners.push(ListenerInfo {
                    port: *port,
                    process: Some(String::from_utf8_lossy(&output.stdout).to_string()),
                    belongs_to_target: false,
                });
            }
        }
        Ok(listeners)
    }

    fn published_port_owner(&self, port: u16) -> Result<Option<String>> {
        let publish_filter = format!("publish={port}");
        let output = run_inspector_command(
            "docker",
            &[
                "ps",
                "--filter",
                &publish_filter,
                "--format",
                "{{.ID}}\t{{.Names}}",
            ],
            Duration::from_secs(10),
        )
        .map_err(|e| DockerUnavailableError(format!("docker ps failed: {e}")))?;
        if !output.status.success() {
            return Err(DockerUnavailableError(format!(
                "docker ps failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
            .into());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut lines = stdout.lines();
        let Some(first) = lines.next() else {
            return Ok(None);
        };
        if lines.next().is_some() {
            return Ok(None);
        }
        let mut fields = first.split('\t');
        let id = fields.next().unwrap_or_default().trim();
        let name = fields.next().unwrap_or_default().trim();
        if !id.is_empty() {
            Ok(Some(id.into()))
        } else if !name.is_empty() {
            Ok(Some(name.into()))
        } else {
            Ok(None)
        }
    }
}

fn ss_output_has_listener(stdout: &[u8]) -> bool {
    String::from_utf8_lossy(stdout)
        .lines()
        .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with("Netid "))
}

fn systemd_status_from_output(
    unit: &str,
    output: &std::process::Output,
) -> Result<Option<SystemdStatus>> {
    if output.status.success() {
        return Ok(Some(SystemdStatus {
            unit: unit.into(),
            active: true,
        }));
    }

    let code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_lowercase();
    if matches!(code, Some(3) | Some(4))
        || matches!(stdout.as_str(), "inactive" | "failed" | "unknown")
    {
        return Ok(Some(SystemdStatus {
            unit: unit.into(),
            active: false,
        }));
    }

    Err(anyhow!(
        "systemctl --user is-active {unit} failed (code={code:?}): {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn run_inspector_command(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<std::process::Output> {
    let timeout_secs = timeout.as_secs().max(1).to_string();
    let mut timeout_args = vec!["-k", "1s", &timeout_secs, program];
    timeout_args.extend(args);
    let output = std::process::Command::new("timeout")
        .args(timeout_args)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow!("'timeout' binary not found; please install GNU coreutils or add timeout to PATH before running compose diagnostics")
            } else {
                anyhow!("failed to run {program} inspector command: {e}")
            }
        })?;
    if program == "systemctl"
        && args.first() == Some(&"--user")
        && !output.status.success()
        && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none()
        && systemctl_needs_user_bus_fallback(&output)
    {
        if let Some((runtime_dir, bus_address)) = inferred_user_bus_env() {
            let mut retry_args = vec!["-k", "1s", &timeout_secs, program];
            retry_args.extend(args);
            return std::process::Command::new("timeout")
                .env("XDG_RUNTIME_DIR", runtime_dir)
                .env("DBUS_SESSION_BUS_ADDRESS", bus_address)
                .args(retry_args)
                .output()
                .map_err(|e| anyhow!("failed to run {program} inspector command: {e}"));
        }
    }
    Ok(output)
}

fn systemctl_needs_user_bus_fallback(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr.contains("DBUS_SESSION_BUS_ADDRESS") || stderr.contains("user scope bus")
}

fn inferred_user_bus_env() -> Option<(PathBuf, String)> {
    let runtime_dir = PathBuf::from(format!("/run/user/{}", current_uid()));
    let bus = runtime_dir.join("bus");
    bus.exists()
        .then(|| (runtime_dir, format!("unix:path={}", bus.display())))
}

fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn container_info_from_inspect(value: serde_json::Value) -> Result<ContainerInfo> {
    let labels = value
        .pointer("/Config/Labels")
        .and_then(|v| v.as_object())
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let name = value
        .get("Name")
        .and_then(|v| v.as_str())
        .unwrap_or("syslog-mcp")
        .trim_start_matches('/')
        .to_string();
    let mounts = value
        .get("Mounts")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|m| MountInfo {
                    source: m.get("Source").and_then(|v| v.as_str()).map(PathBuf::from),
                    target: m
                        .get("Destination")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    kind: m
                        .get("Type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ContainerInfo {
        id: value
            .get("Id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        name,
        status: value
            .pointer("/State/Status")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        health: value
            .pointer("/State/Health/Status")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        image: value
            .pointer("/Config/Image")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        image_id: value
            .get("Image")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        labels,
        mounts,
        ports: ports_from_inspect(&value),
    })
}

fn ports_from_inspect(value: &serde_json::Value) -> Vec<PortInfo> {
    let Some(map) = value
        .pointer("/NetworkSettings/Ports")
        .and_then(|v| v.as_object())
    else {
        return Vec::new();
    };
    let mut ports = Vec::new();
    for (private, bindings) in map {
        let Some((port, protocol)) = private.split_once('/') else {
            continue;
        };
        let Ok(private_port) = port.parse::<u16>() else {
            continue;
        };
        match bindings {
            serde_json::Value::Array(items) if !items.is_empty() => {
                for item in items {
                    ports.push(PortInfo {
                        private_port,
                        public_port: item
                            .get("HostPort")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<u16>().ok()),
                        protocol: protocol.to_string(),
                        host_ip: item
                            .get("HostIp")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                    });
                }
            }
            _ => ports.push(PortInfo {
                private_port,
                public_port: None,
                protocol: protocol.to_string(),
                host_ip: None,
            }),
        }
    }
    ports
}

pub struct ProcessRunner;

impl CommandRunner for ProcessRunner {
    fn run(&self, invocation: &ComposeInvocation) -> Result<CommandOutput> {
        #[cfg(unix)]
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::Instant;

        let mut command = Command::new(&invocation.program);
        command.args(&invocation.args);
        if let Some(dir) = &invocation.current_dir {
            command.current_dir(dir);
        }
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        #[cfg(unix)]
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let mut child = command.spawn().map_err(|e| {
            anyhow!(
                "failed to spawn {} {}: {e}",
                invocation.program,
                invocation.args.join(" ")
            )
        })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("missing stdout pipe"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("missing stderr pipe"))?;
        let stdout_buf = Arc::new(Mutex::new((Vec::new(), false)));
        let stderr_buf = Arc::new(Mutex::new((Vec::new(), false)));

        let out_handle = drain_pipe(
            stdout,
            Arc::clone(&stdout_buf),
            invocation.output_limit_bytes,
        );
        let err_handle = drain_pipe(
            stderr,
            Arc::clone(&stderr_buf),
            invocation.output_limit_bytes,
        );

        let started = Instant::now();
        let mut timed_out = false;
        let mut timeout_cleanup = None;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if started.elapsed() >= invocation.timeout {
                timed_out = true;
                let terminate_sent = terminate_child(&mut child);
                thread::sleep(Duration::from_millis(500));
                let mut kill_sent = false;
                let (status, reaped) = if let Some(status) = child.try_wait()? {
                    (Some(status), true)
                } else {
                    kill_sent = force_kill_child(&mut child);
                    let status = wait_for_child_after_kill(&mut child, Duration::from_secs(2))?;
                    let reaped = status.is_some();
                    (status, reaped)
                };
                timeout_cleanup = Some(TimeoutCleanupStatus {
                    terminate_sent,
                    kill_sent,
                    reaped,
                });
                break status;
            }
            thread::sleep(Duration::from_millis(25));
        };

        if timeout_cleanup.as_ref().map(|c| c.reaped).unwrap_or(true) {
            let _ = out_handle.join();
            let _ = err_handle.join();
        }

        let (stdout, stdout_truncated) = take_buffer(stdout_buf)?;
        let (stderr, stderr_truncated) = take_buffer(stderr_buf)?;

        Ok(CommandOutput {
            exit_status: status.and_then(|status| status.code()),
            stdout: redact_sensitive(&String::from_utf8_lossy(&stdout)),
            stderr: redact_sensitive(&String::from_utf8_lossy(&stderr)),
            stdout_truncated,
            stderr_truncated,
            timed_out,
            timeout_cleanup,
        })
    }
}

fn wait_for_child_after_kill(
    child: &mut std::process::Child,
    cap: Duration,
) -> Result<Option<std::process::ExitStatus>> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if started.elapsed() >= cap {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn drain_pipe<R: std::io::Read + Send + 'static>(
    mut reader: R,
    target: std::sync::Arc<std::sync::Mutex<(Vec<u8>, bool)>>,
    limit: usize,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let mut guard = target.lock().expect("pipe buffer mutex poisoned");
                    append_pipe_chunk(&mut guard, &chunk, n, limit);
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    })
}

fn append_pipe_chunk(target: &mut (Vec<u8>, bool), chunk: &[u8], n: usize, limit: usize) {
    let remaining = limit.saturating_sub(target.0.len());
    if remaining > 0 {
        let keep = remaining.min(n);
        target.0.extend_from_slice(&chunk[..keep]);
        if keep < n {
            target.1 = true;
        }
    } else {
        target.1 = true;
    }
}

fn take_buffer(
    buffer: std::sync::Arc<std::sync::Mutex<(Vec<u8>, bool)>>,
) -> Result<(Vec<u8>, bool)> {
    let guard = buffer
        .lock()
        .map_err(|_| anyhow!("pipe buffer mutex poisoned"))?;
    Ok((guard.0.clone(), guard.1))
}

#[cfg(unix)]
fn terminate_child(child: &mut std::process::Child) -> bool {
    let pid = child.id() as i32;
    unsafe { libc::kill(-pid, libc::SIGTERM) == 0 }
}

#[cfg(unix)]
fn force_kill_child(child: &mut std::process::Child) -> bool {
    let pid = child.id() as i32;
    unsafe { libc::kill(-pid, libc::SIGKILL) == 0 }
}

#[cfg(not(unix))]
fn terminate_child(child: &mut std::process::Child) -> bool {
    child.kill().is_ok()
}

#[cfg(not(unix))]
fn force_kill_child(child: &mut std::process::Child) -> bool {
    child.kill().is_ok()
}

#[cfg(test)]
#[path = "compose_tests.rs"]
mod tests;
