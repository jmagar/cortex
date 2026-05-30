use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

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
            service: "cortex".into(),
            container_name: "cortex".into(),
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
    pub volume_name: Option<String>,
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
    pub labels: std::collections::BTreeMap<String, String>,
    pub mounts: Vec<MountInfo>,
    pub ports: Vec<PortInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerInfo {
    pub port: u16,
    pub process: Option<String>,
    pub belongs_to_target: bool,
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
    fn run(&self, invocation: &ComposeInvocation) -> anyhow::Result<CommandOutput>;
}

pub trait DockerInspect {
    fn inspect_container(&self, name: &str) -> anyhow::Result<Option<ContainerInfo>>;
    fn find_candidates(
        &self,
        service: &str,
        container_name: &str,
    ) -> anyhow::Result<Vec<ContainerInfo>>;
    fn systemd_status(&self, unit: &str) -> anyhow::Result<Option<SystemdStatus>>;
    fn listeners(&self, ports: &[u16]) -> anyhow::Result<Vec<ListenerInfo>>;
    fn published_port_owner(&self, _port: u16) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}
