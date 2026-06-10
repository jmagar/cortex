use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::IpAddr;
use std::path::PathBuf;
use std::str::FromStr;

const MAX_CLEANUP_CHUNK_SIZE: usize = 1_000_000;

/// Wrapper for secret string values that prints `[REDACTED]` in Debug output
/// and is skipped during Serialize so secrets never leak into logs or JSON exports.
#[derive(Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct Secret(pub Option<String>);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            None => write!(f, "None"),
            Some(_) => write!(f, "Some([REDACTED])"),
        }
    }
}

impl Serialize for Secret {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            None => s.serialize_none(),
            Some(_) => s.serialize_str("[REDACTED]"),
        }
    }
}

impl Secret {
    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }
    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }
    pub fn as_ref(&self) -> Option<&String> {
        self.0.as_ref()
    }
}

impl From<Option<String>> for Secret {
    fn from(v: Option<String>) -> Self {
        Self(v)
    }
}

impl From<String> for Secret {
    fn from(v: String) -> Self {
        Self(Some(v))
    }
}

impl PartialEq<Option<String>> for Secret {
    fn eq(&self, other: &Option<String>) -> bool {
        &self.0 == other
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub receiver: ReceiverConfig,
    pub storage: StorageConfig,
    pub mcp: McpConfig,
    pub api: ApiConfig,
    pub docker_ingest: DockerIngestConfig,
    pub enrichment: EnrichmentConfigToml,
    pub error_detection: ErrorDetectionConfig,
    pub notifications: NotificationsConfig,
}

// ---------------------------------------------------------------------------
// Notifications configuration

/// Configuration for the notifications subsystem.
/// Loaded from `[notifications]` in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationsConfig {
    /// Enable push notifications via Apprise. Default: false.
    pub enabled: bool,
    /// Primary Apprise API base URL, e.g. "http://apprise:8000".
    pub apprise_url: String,
    /// List of Apprise notification URLs (e.g. "gotify://host/token").
    pub apprise_urls: Vec<String>,
    /// How often to run the dispatcher loop (seconds). Default: 30.
    pub dispatcher_interval_secs: u64,
    /// Dedup window in seconds — suppress duplicate firings within this
    /// window. Default: 900 (15 minutes).
    pub dedup_window_secs: u64,
    /// Local cron expression for daily digest. Default: "0 8 * * *" (8am).
    pub digest_cron_local: String,
    /// Maximum retry attempts before dead-lettering. Default: 8.
    pub max_retry_attempts: u8,
    /// Per-rule evaluator settings.
    pub evaluators: NotificationEvaluatorsConfig,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            apprise_url: String::new(),
            apprise_urls: Vec::new(),
            dispatcher_interval_secs: 30,
            dedup_window_secs: 900,
            digest_cron_local: "0 8 * * *".to_string(),
            max_retry_attempts: 8,
            evaluators: NotificationEvaluatorsConfig::default(),
        }
    }
}

/// Per-rule toggles for the notification evaluator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationEvaluatorsConfig {
    /// Enable OOM kill detection. Default: true.
    pub oom_kill: bool,
    /// Enable container die non-zero exit detection. Default: true.
    pub container_die_nonzero: bool,
    /// Enable fail2ban ban detection. Default: true.
    pub fail2ban_ban: bool,
    /// Enable Authelia MFA failure detection. Default: true.
    pub authelia_mfa_fail: bool,
    /// Enable disk fill detection from storage guardrail. Default: true.
    pub disk_fill: bool,
    /// Enable ingest queue pressure detection. Default: true.
    pub ingest_queue_pressure: bool,
    /// Enable ingest-silence detection: fire when the DB has logs but the
    /// newest row is older than `ingest_silence_threshold_secs`. This is the
    /// push-path complement to the pull-only `silent_hosts`/`ingest_rate`
    /// actions — a dead listener or broken forwarding chain otherwise goes
    /// unnoticed until someone asks (bead syslog-mcp-7f0y). Default: true.
    pub ingest_silence: bool,
    /// Age of the newest ingested row (seconds) after which ingest is
    /// considered silent. Default: 900 (15 minutes).
    pub ingest_silence_threshold_secs: u64,
    /// How often to run evaluation (seconds). Default: 300 (5 minutes).
    pub evaluator_interval_secs: u64,
}

impl Default for NotificationEvaluatorsConfig {
    fn default() -> Self {
        Self {
            oom_kill: true,
            container_die_nonzero: true,
            fail2ban_ban: true,
            authelia_mfa_fail: true,
            disk_fill: true,
            ingest_queue_pressure: true,
            ingest_silence: true,
            ingest_silence_threshold_secs: 900,
            evaluator_interval_secs: 300,
        }
    }
}

/// Configuration for the background error signature scan job.
/// Loaded from `[error_detection]` in `config.toml` or env vars.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorDetectionConfig {
    /// Enable the background scan job.
    pub enabled: bool,
    /// How often to run the scan cycle (seconds). Default: 3600 (1 hour).
    pub scan_interval_secs: u64,
    /// Maximum log rows to scan per cycle. Default: 50_000.
    pub max_rows_per_cycle: u32,
    /// Minimum count of firings in a 1h window before a signature is
    /// considered "notable". Default: 30.
    pub frequency_threshold: u32,
}

impl Default for ErrorDetectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            scan_interval_secs: 3600,
            max_rows_per_cycle: 50_000,
            frequency_threshold: 30,
        }
    }
}

/// Enrichment + scrubbing knobs. Loaded from `[enrichment]` in `config.toml`
/// or from `CORTEX_*` env vars at runtime startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EnrichmentConfigToml {
    /// If set, only apply Authelia severity reclassification when an entry's
    /// `source_ip` starts with this prefix. Prevents non-Authelia hosts from
    /// spoofing severity by sending crafted messages with `tag=authelia`.
    pub authelia_source_ip: Option<String>,
    /// Same gating, for AdGuard JSON tag classification.
    pub adguard_source_ip: Option<String>,
    /// Best-effort credential scrubbing on AI-source records. Default true.
    /// Set to false only if downstream consumers need raw prompt text and
    /// you trust every tailnet node.
    pub scrub_prompts: bool,
    /// FTS5 incremental-merge page budget per call. Each merge processes at most
    /// this many index pages and then returns, keeping the write lock held only
    /// briefly. 0 maps to the built-in default (`DEFAULT_FTS_MERGE_PAGES`, 500)
    /// because a 0-page merge is a no-op in FTS5's `VALUES('merge', N)` API.
    /// Raise it to reclaim phantom space faster after large deletes; lower it if
    /// merges hold the write lock too long on a very large index. Range: 0..=10000.
    pub fts_merge_pages: u32,
}

impl Default for EnrichmentConfigToml {
    fn default() -> Self {
        Self {
            authelia_source_ip: None,
            adguard_source_ip: None,
            scrub_prompts: true,
            fts_merge_pages: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReceiverConfig {
    /// Listen host (shared by UDP + TCP)
    #[serde(default = "default_receiver_host")]
    pub host: String,
    /// Listen port (shared by UDP + TCP)
    #[serde(default = "default_receiver_port")]
    pub port: u16,
    /// Max message size in bytes
    #[serde(default = "default_max_message_size")]
    pub max_message_size: usize,
    /// Maximum concurrent TCP connections (semaphore cap)
    #[serde(default = "default_max_tcp_connections")]
    pub max_tcp_connections: usize,
    /// Idle timeout in seconds for TCP connections (per-read)
    #[serde(default = "default_tcp_idle_timeout_secs")]
    pub tcp_idle_timeout_secs: u64,
    /// Batch writer: entries per flush
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Batch writer: flush interval in milliseconds
    #[serde(default = "default_flush_interval")]
    pub flush_interval: u64,
    /// Internal parsed-message channel capacity.
    #[serde(default = "default_write_channel_capacity")]
    pub write_channel_capacity: usize,
    /// Optional CIDR allowlist for syslog senders. When non-empty, UDP packets
    /// and TCP connections from addresses not matching any CIDR are silently
    /// dropped. Empty = accept all (default).
    /// Set via `CORTEX_ALLOWED_SOURCE_CIDRS=10.0.0.0/8,192.168.0.0/16`
    #[serde(default)]
    pub allowed_source_cidrs: Vec<String>,
}

impl ReceiverConfig {
    /// Returns "host:port" for binding UDP/TCP listeners.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// Path to SQLite database
    #[serde(default = "default_db_path")]
    pub db_path: PathBuf,
    /// Connection pool size
    #[serde(default = "default_pool_size")]
    pub pool_size: u32,
    /// Days to retain logs before automatic deletion (0 = keep forever).
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    /// WAL mode (recommended for concurrent reads)
    #[serde(default = "default_true")]
    pub wal_mode: bool,
    /// Soft limit for logical DB size in MB (0 = disabled)
    #[serde(default = "default_max_db_size_mb")]
    pub max_db_size_mb: u64,
    /// Recovery target for logical DB size in MB
    #[serde(default = "default_recovery_db_size_mb")]
    pub recovery_db_size_mb: u64,
    /// Minimum free disk in MB for the DB filesystem (0 = disabled)
    #[serde(default = "default_min_free_disk_mb")]
    pub min_free_disk_mb: u64,
    /// Recovery target for free disk in MB
    #[serde(default = "default_recovery_free_disk_mb")]
    pub recovery_free_disk_mb: u64,
    /// Storage budget enforcement interval in seconds
    #[serde(default = "default_cleanup_interval_secs")]
    pub cleanup_interval_secs: u64,
    /// Number of rows to delete per chunk during storage enforcement
    #[serde(default = "default_cleanup_chunk_size")]
    pub cleanup_chunk_size: usize,
    /// Time window (hours) during which high-severity (err/crit/alert/emerg) logs
    /// are protected from disk-pressure deletion. 0 = disable the err+ floor.
    #[serde(default = "default_err_floor_window_hours")]
    pub err_floor_window_hours: u64,
    /// Maximum err+ rows protected per source IP within the floor window. Bounds
    /// any single source's share of the protected set. 0 = disable the floor.
    #[serde(default = "default_err_floor_per_source_cap")]
    pub err_floor_per_source_cap: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    /// HTTP listen host
    #[serde(default = "default_mcp_host")]
    pub host: String,
    /// HTTP listen port
    #[serde(default = "default_mcp_port")]
    pub port: u16,
    /// Server name exposed via MCP
    #[serde(default = "default_server_name")]
    pub server_name: String,
    /// Explicitly disable MCP auth. Without `trusted_gateway_no_auth`, this is
    /// only legal on loopback binds.
    #[serde(default)]
    pub no_auth: bool,
    /// Permit `no_auth` on non-loopback binds only when an upstream gateway
    /// enforces authentication before requests reach cortex.
    #[serde(default)]
    pub trusted_gateway_no_auth: bool,
    /// Optional bearer token for authenticating MCP requests.
    #[serde(default)]
    pub api_token: Secret,
    /// Optional additional Host header values accepted by RMCP Host validation.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    /// Optional browser Origin values accepted by RMCP Origin validation.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// OAuth / JWT authentication policy (consumed by lab-auth at runtime).
    #[serde(default)]
    pub auth: AuthConfig,
    /// When `true`, the static bearer token (`CORTEX_TOKEN`) is granted
    /// both `cortex:read` and `cortex:admin` scopes. Default is `false` —
    /// static tokens receive `cortex:read` only, matching OAuth read-only
    /// tokens. Set `CORTEX_STATIC_TOKEN_ADMIN=true` or the TOML field
    /// `[mcp] static_token_is_admin = true` to opt in to admin grant.
    ///
    /// # Security
    /// Admin actions (VACUUM, Apprise test, error-sig ack/unack) are
    /// high-impact. Sharing a read-only token with a collaborator should not
    /// implicitly grant those operations.
    #[serde(default)]
    pub static_token_is_admin: bool,
}

/// Authentication mode for the MCP HTTP endpoint.
///
/// `Bearer` (default) preserves the legacy single static-token flow. `OAuth`
/// activates the dual-mode middleware shipped by `lab-auth` (Google-issued
/// JWTs with optional static-token coexistence governed by
/// [`AuthConfig::disable_static_token_with_oauth`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    #[default]
    Bearer,
    OAuth,
}

/// `[mcp.auth]` policy table. Core deployment secrets and the bootstrap admin
/// can be provided through env vars so container deployments do not have to
/// mount a TOML file with credentials or site-local identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// Runtime mode toggle. Defaults to `bearer`; set to `oauth` to activate
    /// the dual-mode middleware. Overridable via `CORTEX_AUTH_MODE`.
    #[serde(default)]
    pub mode: AuthMode,
    /// Base URL the OAuth issuer + audience are derived from. Required when
    /// `mode == OAuth`. Overridable via `CORTEX_PUBLIC_URL`.
    #[serde(default)]
    pub public_url: Option<String>,
    /// Google OAuth client id. Required when `mode == OAuth`. Overridable via
    /// `CORTEX_GOOGLE_CLIENT_ID`.
    #[serde(default)]
    pub google_client_id: Option<String>,
    /// Google OAuth client secret. Required when `mode == OAuth`. Overridable
    /// via `CORTEX_GOOGLE_CLIENT_SECRET`.
    #[serde(default)]
    pub google_client_secret: Secret,
    /// Single admin email permitted to log in via Google OAuth. Overridable via
    /// `CORTEX_AUTH_ADMIN_EMAIL`.
    #[serde(default)]
    pub admin_email: String,
    /// Future multi-user email allowlist. Parsed for schema compatibility, but
    /// rejected as the only OAuth gate until lab-auth enforces it.
    #[serde(default)]
    pub allowed_emails: Vec<String>,
    /// Path to the auth SQLite store. Relative paths are resolved against the
    /// directory containing `[storage].db_path` at runtime startup.
    #[serde(default = "default_auth_sqlite_path")]
    pub sqlite_path: PathBuf,
    /// Path to the JWT signing key (PEM). Relative paths are resolved against
    /// the directory containing `[storage].db_path` at runtime startup.
    #[serde(default = "default_auth_key_path")]
    pub key_path: PathBuf,
    /// Access-token TTL in seconds (default 1h).
    #[serde(default = "default_access_token_ttl_secs")]
    pub access_token_ttl_secs: u64,
    /// Refresh-token TTL in seconds (default 8h, deliberately shorter than
    /// lab-auth's 30d for the read-only homelab profile).
    #[serde(default = "default_refresh_token_ttl_secs")]
    pub refresh_token_ttl_secs: u64,
    /// Authorization-code TTL in seconds (default 5m).
    #[serde(default = "default_auth_code_ttl_secs")]
    pub auth_code_ttl_secs: u64,
    /// Per-process rate limit on `/register`. Moot for cortex (the
    /// bearer-only router defined in lab-auth's L2 work does not mount
    /// `/register`) but kept for lab-auth signature parity.
    #[serde(default = "default_register_rpm")]
    pub register_rpm: u32,
    /// Per-process rate limit on `/authorize`.
    #[serde(default = "default_authorize_rpm")]
    pub authorize_rpm: u32,
    /// When `mode == OAuth`, also reject the static `CORTEX_TOKEN`. Set
    /// `false` to keep the static token as a break-glass path. Default `true`.
    /// Overridable via `CORTEX_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH`.
    #[serde(default = "default_true")]
    pub disable_static_token_with_oauth: bool,
    /// Allowed redirect URIs for OAuth clients (loopback URIs are accepted
    /// implicitly by lab-auth; entries here are non-loopback URIs).
    /// Overridable via `CORTEX_AUTH_ALLOWED_REDIRECT_URIS`.
    #[serde(default)]
    pub allowed_client_redirect_uris: Vec<String>,
}

impl McpConfig {
    /// Returns "host:port" for binding the MCP HTTP server.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ApiConfig {
    /// Required bearer token for the always-on non-MCP JSON API.
    /// Provisioned by `cortex setup repair`. The server fails to start without it.
    #[serde(default)]
    pub api_token: Secret,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DockerIngestConfig {
    /// Enable remote Docker log ingestion through docker-socket-proxy endpoints.
    #[serde(default)]
    pub enabled: bool,
    /// Remote Docker hosts to ingest from.
    #[serde(default)]
    pub hosts: Vec<DockerHostConfig>,
    /// Container names to skip across all Docker ingest hosts.
    #[serde(default)]
    pub excluded_containers: Vec<String>,
    /// Initial reconnect backoff in milliseconds per Docker host.
    #[serde(default = "default_docker_reconnect_initial_ms")]
    pub reconnect_initial_ms: u64,
    /// Maximum reconnect backoff in milliseconds per Docker host.
    #[serde(default = "default_docker_reconnect_max_ms")]
    pub reconnect_max_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerHostConfig {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub allow_insecure_http: bool,
    #[serde(default)]
    pub excluded_containers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DockerHostsFile {
    hosts: Vec<DockerHostConfig>,
}

// --- Defaults ---

fn default_receiver_host() -> String {
    "0.0.0.0".into()
}
fn default_receiver_port() -> u16 {
    1514
}
fn default_db_path() -> PathBuf {
    PathBuf::from("/data/cortex.db")
}
fn default_mcp_host() -> String {
    // Loopback by default — operators exposing the query API to a network
    // interface set CORTEX_HOST=0.0.0.0 (or bind a reverse proxy).
    // The syslog ingest port (1514) intentionally keeps 0.0.0.0 because
    // log receivers need to accept from all sending hosts.
    "127.0.0.1".into()
}
fn default_mcp_port() -> u16 {
    3100
}
fn default_max_message_size() -> usize {
    8192
}
fn default_max_tcp_connections() -> usize {
    512
}
fn default_tcp_idle_timeout_secs() -> u64 {
    300
}
fn default_batch_size() -> usize {
    100
}
fn default_flush_interval() -> u64 {
    500
}
fn default_write_channel_capacity() -> usize {
    10_000
}
fn default_pool_size() -> u32 {
    4
}
fn default_retention_days() -> u32 {
    90
}
fn default_max_db_size_mb() -> u64 {
    1024
}
fn default_recovery_db_size_mb() -> u64 {
    900
}
fn default_min_free_disk_mb() -> u64 {
    // 0 = disabled. Whole-filesystem free space is an EXTERNAL condition cortex
    // cannot resolve by deleting its OWN data — see syslog-mcp-w4hh. When this is
    // non-zero the enforcement path treats low free disk as a write-block signal,
    // NOT as a trigger to self-trim. Default 0 to stop the self-wipe out of the box.
    0
}
fn default_recovery_free_disk_mb() -> u64 {
    // MUST stay paired with default_min_free_disk_mb: validate_storage_config
    // requires recovery_free_disk_mb == 0 when min_free_disk_mb == 0, so a fresh
    // StorageConfig::default() would FAIL validation if this were non-zero.
    0
}
fn default_err_floor_window_hours() -> u64 {
    // err+ rows received within this window are protected from disk-pressure
    // deletion. Time-windowed (not unbounded) so an unauthenticated source cannot
    // pin the floor indefinitely with severity=err spam (syslog-mcp-w4hh W1).
    24
}
fn default_err_floor_per_source_cap() -> usize {
    // Maximum err+ rows protected per source IP within the window. Bounds how much
    // of the protected set any single (attacker-controlled) source can occupy, so
    // one hostile sender cannot monopolise the floor (syslog-mcp-w4hh W1).
    10_000
}
fn default_cleanup_interval_secs() -> u64 {
    60
}
fn default_cleanup_chunk_size() -> usize {
    2_000
}
fn default_true() -> bool {
    true
}
fn default_server_name() -> String {
    "cortex".into()
}
fn default_docker_reconnect_initial_ms() -> u64 {
    1_000
}
fn default_docker_reconnect_max_ms() -> u64 {
    30_000
}
fn default_auth_sqlite_path() -> PathBuf {
    PathBuf::from("auth.db")
}
fn default_auth_key_path() -> PathBuf {
    PathBuf::from("auth-jwt.pem")
}
fn default_access_token_ttl_secs() -> u64 {
    3_600 // 1h
}
fn default_refresh_token_ttl_secs() -> u64 {
    28_800 // 8h
}
fn default_auth_code_ttl_secs() -> u64 {
    300 // 5m
}
fn default_register_rpm() -> u32 {
    20
}
fn default_authorize_rpm() -> u32 {
    60
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self {
            host: default_receiver_host(),
            port: default_receiver_port(),
            max_message_size: default_max_message_size(),
            max_tcp_connections: default_max_tcp_connections(),
            tcp_idle_timeout_secs: default_tcp_idle_timeout_secs(),
            batch_size: default_batch_size(),
            flush_interval: default_flush_interval(),
            write_channel_capacity: default_write_channel_capacity(),
            allowed_source_cidrs: Vec::new(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            db_path: default_db_path(),
            pool_size: default_pool_size(),
            retention_days: default_retention_days(),
            wal_mode: true,
            max_db_size_mb: default_max_db_size_mb(),
            recovery_db_size_mb: default_recovery_db_size_mb(),
            min_free_disk_mb: default_min_free_disk_mb(),
            recovery_free_disk_mb: default_recovery_free_disk_mb(),
            cleanup_interval_secs: default_cleanup_interval_secs(),
            cleanup_chunk_size: default_cleanup_chunk_size(),
            err_floor_window_hours: default_err_floor_window_hours(),
            err_floor_per_source_cap: default_err_floor_per_source_cap(),
        }
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            host: default_mcp_host(),
            port: default_mcp_port(),
            server_name: default_server_name(),
            no_auth: false,
            trusted_gateway_no_auth: false,
            api_token: Secret(None),
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
            auth: AuthConfig::default(),
            static_token_is_admin: false,
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            mode: AuthMode::default(),
            public_url: None,
            google_client_id: None,
            google_client_secret: Secret(None),
            admin_email: String::new(),
            allowed_emails: Vec::new(),
            sqlite_path: default_auth_sqlite_path(),
            key_path: default_auth_key_path(),
            access_token_ttl_secs: default_access_token_ttl_secs(),
            refresh_token_ttl_secs: default_refresh_token_ttl_secs(),
            auth_code_ttl_secs: default_auth_code_ttl_secs(),
            register_rpm: default_register_rpm(),
            authorize_rpm: default_authorize_rpm(),
            disable_static_token_with_oauth: true,
            allowed_client_redirect_uris: Vec::new(),
        }
    }
}

impl Default for DockerIngestConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hosts: Vec::new(),
            excluded_containers: Vec::new(),
            reconnect_initial_ms: default_docker_reconnect_initial_ms(),
            reconnect_max_ms: default_docker_reconnect_max_ms(),
        }
    }
}

impl Config {
    /// Load config for stdio / query-only mode.
    ///
    /// Identical to [`Config::load`] but skips the non-loopback bind safety
    /// gate in `validate_auth_config`. In stdio mode cortex never binds an
    /// HTTP port, so the gate is irrelevant and would falsely reject
    /// configurations like `mcp.host = "0.0.0.0"` that are valid for the HTTP
    /// server but harmless in stdio mode.
    pub fn load_for_stdio() -> anyhow::Result<Self> {
        Self::load_inner(false)
    }

    pub fn load() -> anyhow::Result<Self> {
        Self::load_inner(true)
    }

    fn load_inner(check_bind: bool) -> anyhow::Result<Self> {
        // 1. Start with defaults
        let mut config = Config::default();

        // 2. Overlay config.toml if present (partial configs are supported — missing
        //    fields keep their defaults from step 1 via #[serde(default)] annotations)
        match std::fs::read_to_string("config.toml") {
            Ok(contents) => {
                config = toml::from_str(&contents)
                    .map_err(|e| anyhow::anyhow!("Failed to parse config.toml: {e}"))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(anyhow::anyhow!("Failed to read config.toml: {e}")),
        }

        #[cfg(not(test))]
        load_setup_env_file();

        // 3. Overlay environment variables (highest priority)
        //    CORTEX_*     → syslog listener settings
        //    CORTEX_* → MCP server + storage settings
        env_override_str("CORTEX_RECEIVER_HOST", &mut config.receiver.host);
        env_override_parse("CORTEX_RECEIVER_PORT", &mut config.receiver.port)?;
        env_override_parse(
            "CORTEX_MAX_MESSAGE_SIZE",
            &mut config.receiver.max_message_size,
        )?;
        env_override_parse(
            "CORTEX_MAX_TCP_CONNECTIONS",
            &mut config.receiver.max_tcp_connections,
        )?;
        env_override_parse(
            "CORTEX_TCP_IDLE_TIMEOUT_SECS",
            &mut config.receiver.tcp_idle_timeout_secs,
        )?;
        env_override_parse("CORTEX_BATCH_SIZE", &mut config.receiver.batch_size)?;
        env_override_parse("CORTEX_FLUSH_INTERVAL", &mut config.receiver.flush_interval)?;
        env_override_parse(
            "CORTEX_WRITE_CHANNEL_CAPACITY",
            &mut config.receiver.write_channel_capacity,
        )?;
        env_override_list(
            "CORTEX_ALLOWED_SOURCE_CIDRS",
            &mut config.receiver.allowed_source_cidrs,
        );

        env_override_str("CORTEX_HOST", &mut config.mcp.host);
        env_override_parse("CORTEX_PORT", &mut config.mcp.port)?;
        env_override_bool("NO_AUTH", &mut config.mcp.no_auth)?;
        env_override_bool("CORTEX_NO_AUTH", &mut config.mcp.no_auth)?;
        env_override_bool(
            "CORTEX_TRUSTED_GATEWAY_NO_AUTH",
            &mut config.mcp.trusted_gateway_no_auth,
        )?;
        env_override_bool(
            "CORTEX_STATIC_TOKEN_ADMIN",
            &mut config.mcp.static_token_is_admin,
        )?;
        env_override_list("CORTEX_ALLOWED_HOSTS", &mut config.mcp.allowed_hosts);
        env_override_list("CORTEX_ALLOWED_ORIGINS", &mut config.mcp.allowed_origins);
        // MCP static token. (The pre-v1 `SYSLOG_MCP_API_TOKEN` deprecated alias was
        // dropped in the cortex v1.0.0 rebrand — its post-rename name `CORTEX_API_TOKEN`
        // now belongs exclusively to the API/OTLP token, `config.api.api_token`.)
        env_override_opt_str("CORTEX_TOKEN", &mut config.mcp.api_token.0);
        env_override_path("CORTEX_DB_PATH", &mut config.storage.db_path);
        // Fail fast when CORTEX_DB_PATH is explicitly set but its parent
        // directory doesn't exist. This catches the common Docker misconfiguration
        // where the variable is set to a host filesystem path that was never
        // bind-mounted into the container, producing a cryptic "Permission denied"
        // error deep in SQLite pool initialisation.
        if std::env::var_os("CORTEX_DB_PATH").is_some() {
            if let Some(parent) = config.storage.db_path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    anyhow::bail!(
                        "CORTEX_DB_PATH parent directory does not exist: {}\n\
                         In Docker: mount the data directory at /data and set\n\
                         CORTEX_DB_PATH=/data/cortex.db",
                        parent.display()
                    );
                }
            }
        }
        env_override_parse("CORTEX_POOL_SIZE", &mut config.storage.pool_size)?;
        env_override_parse("CORTEX_RETENTION_DAYS", &mut config.storage.retention_days)?;
        env_override_parse("CORTEX_MAX_DB_SIZE_MB", &mut config.storage.max_db_size_mb)?;
        env_override_parse(
            "CORTEX_RECOVERY_DB_SIZE_MB",
            &mut config.storage.recovery_db_size_mb,
        )?;
        env_override_parse(
            "CORTEX_MIN_FREE_DISK_MB",
            &mut config.storage.min_free_disk_mb,
        )?;
        env_override_parse(
            "CORTEX_RECOVERY_FREE_DISK_MB",
            &mut config.storage.recovery_free_disk_mb,
        )?;
        env_override_parse(
            "CORTEX_CLEANUP_INTERVAL_SECS",
            &mut config.storage.cleanup_interval_secs,
        )?;
        env_override_parse(
            "CORTEX_CLEANUP_CHUNK_SIZE",
            &mut config.storage.cleanup_chunk_size,
        )?;
        env_override_parse(
            "CORTEX_ERR_FLOOR_WINDOW_HOURS",
            &mut config.storage.err_floor_window_hours,
        )?;
        env_override_parse(
            "CORTEX_ERR_FLOOR_PER_SOURCE_CAP",
            &mut config.storage.err_floor_per_source_cap,
        )?;

        // [mcp.auth] env overrides.
        env_override_auth_mode("CORTEX_AUTH_MODE", &mut config.mcp.auth.mode)?;
        env_override_opt_str("CORTEX_PUBLIC_URL", &mut config.mcp.auth.public_url);
        env_override_opt_str(
            "CORTEX_GOOGLE_CLIENT_ID",
            &mut config.mcp.auth.google_client_id,
        );
        env_override_opt_str(
            "CORTEX_GOOGLE_CLIENT_SECRET",
            &mut config.mcp.auth.google_client_secret.0,
        );
        env_override_str("CORTEX_AUTH_ADMIN_EMAIL", &mut config.mcp.auth.admin_email);
        env_override_list(
            "CORTEX_AUTH_ALLOWED_REDIRECT_URIS",
            &mut config.mcp.auth.allowed_client_redirect_uris,
        );
        env_override_bool(
            "CORTEX_AUTH_DISABLE_STATIC_TOKEN_WITH_OAUTH",
            &mut config.mcp.auth.disable_static_token_with_oauth,
        )?;

        env_override_opt_str("CORTEX_API_TOKEN", &mut config.api.api_token.0);

        env_override_opt_str(
            "CORTEX_AUTHELIA_SOURCE_IP",
            &mut config.enrichment.authelia_source_ip,
        );
        env_override_opt_str(
            "CORTEX_ADGUARD_SOURCE_IP",
            &mut config.enrichment.adguard_source_ip,
        );
        env_override_bool("CORTEX_SCRUB_PROMPTS", &mut config.enrichment.scrub_prompts)?;
        env_override_parse(
            "CORTEX_FTS_MERGE_PAGES",
            &mut config.enrichment.fts_merge_pages,
        )?;
        if config.enrichment.fts_merge_pages > 10_000 {
            return Err(anyhow::anyhow!(
                "CORTEX_FTS_MERGE_PAGES must be in 0..=10000, got {}",
                config.enrichment.fts_merge_pages
            ));
        }

        env_override_bool(
            "CORTEX_ERROR_DETECTION_ENABLED",
            &mut config.error_detection.enabled,
        )?;
        env_override_parse(
            "CORTEX_ERROR_DETECTION_SCAN_INTERVAL_SECS",
            &mut config.error_detection.scan_interval_secs,
        )?;
        env_override_bool(
            "CORTEX_NOTIFICATIONS_ENABLED",
            &mut config.notifications.enabled,
        )?;
        env_override_str(
            "CORTEX_NOTIFICATIONS_APPRISE_URL",
            &mut config.notifications.apprise_url,
        );

        env_override_bool(
            "CORTEX_DOCKER_INGEST_ENABLED",
            &mut config.docker_ingest.enabled,
        )?;
        env_override_parse(
            "CORTEX_DOCKER_RECONNECT_INITIAL_MS",
            &mut config.docker_ingest.reconnect_initial_ms,
        )?;
        env_override_parse(
            "CORTEX_DOCKER_RECONNECT_MAX_MS",
            &mut config.docker_ingest.reconnect_max_ms,
        )?;
        env_override_list(
            "CORTEX_DOCKER_EXCLUDED_CONTAINERS",
            &mut config.docker_ingest.excluded_containers,
        );
        if config.docker_ingest.enabled {
            if let Ok(val) = std::env::var("CORTEX_DOCKER_HOSTS") {
                if !val.is_empty() {
                    config.docker_ingest.hosts = val
                        .split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|name| DockerHostConfig {
                            name: name.to_string(),
                            base_url: format!("http://{}:2375", name),
                            allow_insecure_http: true,
                            excluded_containers: Vec::new(),
                        })
                        .collect();
                    for host in &config.docker_ingest.hosts {
                        tracing::warn!(
                            host = %host.name,
                            base_url = %host.base_url,
                            "CORTEX_DOCKER_HOSTS expands to insecure HTTP docker-socket-proxy endpoints; use only on trusted private networks or CORTEX_DOCKER_HOSTS_FILE with TLS/custom base_url"
                        );
                    }
                }
            } else if let Ok(path) = std::env::var("CORTEX_DOCKER_HOSTS_FILE") {
                if !path.is_empty() {
                    match std::fs::read_to_string(&path) {
                        Ok(contents) => {
                            let parsed: DockerHostsFile =
                                toml::from_str(&contents).map_err(|e| {
                                    anyhow::anyhow!(
                                        "Failed to parse CORTEX_DOCKER_HOSTS_FILE={path}: {e}"
                                    )
                                })?;
                            config.docker_ingest.hosts = parsed.hosts;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            tracing::warn!(
                                path = %path,
                                "CORTEX_DOCKER_HOSTS_FILE not found — no docker hosts loaded. \
                                 Create the file or use CORTEX_DOCKER_HOSTS instead."
                            );
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!(
                                "Failed to read CORTEX_DOCKER_HOSTS_FILE={path}: {e}"
                            ));
                        }
                    }
                }
            }
        }

        // Validation
        if config.storage.pool_size == 0 {
            return Err(anyhow::anyhow!("CORTEX_POOL_SIZE must be > 0"));
        }
        validate_receiver_config(&config.receiver)?;
        validate_storage_config(&config.storage)?;
        validate_notifications_config(&config.notifications)?;
        validate_error_detection_config(&config.error_detection)?;
        validate_host(&config.receiver.host)?;
        validate_host(&config.mcp.host)?;
        validate_auth_config(&config, check_bind)?;
        validate_docker_ingest_config(&config.docker_ingest)?;

        Ok(config)
    }
}

#[cfg(not(test))]
fn load_setup_env_file() {
    let Ok(home) = crate::setup::cortex_home_dir() else {
        tracing::trace!("load_setup_env_file: syslog home directory unavailable");
        return;
    };
    let path = home.join(".env");
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
        tracing::trace!(path = %path.display(), "load_setup_env_file: env file metadata unavailable");
        return;
    };
    if metadata.file_type().is_symlink() {
        tracing::trace!(path = %path.display(), "load_setup_env_file: refusing symlinked env file");
        eprintln!(
            "cortex: warning: refusing to load symlinked env file {}",
            path.display()
        );
        return;
    }
    let Ok(raw) = std::fs::read_to_string(&path) else {
        tracing::trace!(path = %path.display(), "load_setup_env_file: env file read failed");
        return;
    };
    let mut entries = Vec::new();
    for (line_no, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            tracing::trace!("load_setup_env_file: skipped blank/comment line");
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            tracing::trace!(
                line_no = line_no + 1,
                "load_setup_env_file: skipped line without delimiter"
            );
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || key.contains(['\0']) || value.contains(['\0']) {
            tracing::trace!(key, "load_setup_env_file: skipped invalid env entry");
            continue;
        }
        if !is_supported_setup_env_key(key) {
            tracing::trace!(key, "load_setup_env_file: skipped unsupported env key");
            continue;
        }
        entries.push((key.to_string(), value.to_string()));
    }

    let data_volume = entries
        .iter()
        .find(|(key, _)| key == "CORTEX_DATA_VOLUME")
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(_, value)| value.clone());
    if let Some(data_volume) = data_volume.as_deref() {
        tracing::trace!(data_volume, "load_setup_env_file: found CORTEX_DATA_VOLUME");
    }

    for (key, mut value) in entries {
        if std::env::var_os(&key).is_some() {
            tracing::trace!(key, "load_setup_env_file: process env already set");
            continue;
        }
        if key == "CORTEX_DB_PATH" {
            if let Some(suffix) = value.strip_prefix("/data/") {
                if let Some(data_volume) = data_volume.as_deref() {
                    value = PathBuf::from(data_volume)
                        .join(suffix)
                        .display()
                        .to_string();
                    tracing::trace!(value, "load_setup_env_file: rewrote CORTEX_DB_PATH");
                }
            }
        }
        tracing::trace!(key, "load_setup_env_file: setting env entry");
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var(key, value) };
    }
}

#[cfg(not(test))]
fn is_supported_setup_env_key(key: &str) -> bool {
    key == "NO_AUTH"
        || key.starts_with("CORTEX_")
        || key.starts_with("CORTEX_")
        || key.starts_with("CORTEX_API_")
        || key.starts_with("CORTEX_DOCKER_")
}

// --- Env var helpers ---

fn env_override_str(key: &str, target: &mut String) {
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            *target = v;
        }
    }
}

fn env_override_opt_str(key: &str, target: &mut Option<String>) {
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            *target = Some(v);
        }
    }
}

fn env_override_path(key: &str, target: &mut PathBuf) {
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            *target = PathBuf::from(v);
        }
    }
}

fn env_override_list(key: &str, target: &mut Vec<String>) {
    let Ok(v) = std::env::var(key) else {
        return;
    };
    let values: Vec<String> = v
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    *target = values;
}

fn env_override_auth_mode(key: &str, target: &mut AuthMode) -> anyhow::Result<()> {
    let Ok(v) = std::env::var(key) else {
        return Ok(());
    };
    if v.is_empty() {
        return Ok(());
    }
    *target = match v.trim().to_ascii_lowercase().as_str() {
        "bearer" => AuthMode::Bearer,
        "oauth" => AuthMode::OAuth,
        other => {
            return Err(anyhow::anyhow!(
                "Invalid value for {key}={other}: expected `bearer` or `oauth`"
            ));
        }
    };
    Ok(())
}

fn env_override_bool(key: &str, target: &mut bool) -> anyhow::Result<()> {
    let Ok(v) = std::env::var(key) else {
        return Ok(());
    };
    if v.is_empty() {
        return Ok(());
    }

    *target = match v.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" | "on" => true,
        "false" | "0" | "no" | "n" | "off" => false,
        _ => {
            return Err(anyhow::anyhow!(
                "Invalid value for {key}={v}: expected true/false/1/0/yes/no/on/off"
            ));
        }
    };
    Ok(())
}

pub(crate) fn validate_auth_config(config: &Config, check_bind: bool) -> anyhow::Result<()> {
    if token_is_set_but_blank(&config.mcp.api_token.0) {
        return Err(anyhow::anyhow!("mcp.api_token must not be empty"));
    }
    if token_is_set_but_blank(&config.api.api_token.0) {
        return Err(anyhow::anyhow!("api.api_token must not be empty"));
    }
    // Note: CORTEX_API_TOKEN being entirely unset is enforced at
    // route-mount time by `api::router` (anyhow::bail) rather than here.
    // Failing in `Config::load()` would break stdio-mode invocations
    // (which call `load_for_stdio()` with check_bind=false but still hit
    // this function) and the broad swath of tests that build Config
    // without exporting the token. The route-mount bail still fires
    // during server startup before any request is served, so operators
    // see the same error early.

    // ---- OAuth prerequisites ----------------------------------------------
    let auth = &config.mcp.auth;
    if config.mcp.no_auth {
        let bind_is_loopback = mcp_bind_is_loopback(config);
        if check_bind && !bind_is_loopback && !config.mcp.trusted_gateway_no_auth {
            return Err(anyhow::anyhow!(
                "MCP host `{}` is not a loopback address and CORTEX_NO_AUTH=true was set \
                 without CORTEX_TRUSTED_GATEWAY_NO_AUTH=true. Bind to 127.0.0.1 / ::1, \
                 configure CORTEX_TOKEN or OAuth, or set the trusted-gateway flag only \
                 when an upstream gateway enforces authentication before cortex.",
                config.mcp.host
            ));
        }
        return Ok(());
    }

    if auth.mode == AuthMode::OAuth {
        if option_is_blank(&auth.public_url) {
            return Err(anyhow::anyhow!(
                "CORTEX_PUBLIC_URL is required when CORTEX_AUTH_MODE=oauth — \
                 set the externally reachable base URL (e.g. https://syslog.example.com)"
            ));
        }
        if option_is_blank(&auth.google_client_id) {
            return Err(anyhow::anyhow!(
                "CORTEX_GOOGLE_CLIENT_ID is required when CORTEX_AUTH_MODE=oauth"
            ));
        }
        if option_is_blank(&auth.google_client_secret.0) {
            return Err(anyhow::anyhow!(
                "CORTEX_GOOGLE_CLIENT_SECRET is required when CORTEX_AUTH_MODE=oauth"
            ));
        }
        let admin_blank = auth.admin_email.trim().is_empty();
        let allowed_emails_set = auth
            .allowed_emails
            .iter()
            .any(|entry| !entry.trim().is_empty());
        if allowed_emails_set {
            return Err(anyhow::anyhow!(
                "[mcp.auth].allowed_emails is not passed to lab-auth; remove \
                 `allowed_emails` and use `admin_email` or lab-auth-managed allowed_users until \
                 cortex can enforce the config list"
            ));
        }
        if admin_blank {
            return Err(anyhow::anyhow!(
                "[mcp.auth] requires a non-empty `admin_email` when \
                 CORTEX_AUTH_MODE=oauth — `allowed_emails` is parsed but not passed to \
                 lab-auth yet"
            ));
        }
    }
    // Note: `disable_static_token_with_oauth` defaults to `true`; in pure
    // bearer mode the flag is a no-op (no OAuth path to disable) so we do not
    // reject the default combo. The flag only takes effect at middleware
    // mount time when OAuth is active (S3's job).

    // ---- Non-loopback safety gate -----------------------------------------
    // Skip in stdio / query-only mode: no HTTP port is bound so the gate is
    // irrelevant. `check_bind` is false when called from Config::load_for_stdio.
    let bind_is_loopback = mcp_bind_is_loopback(config);
    if check_bind && !bind_is_loopback {
        let has_static_token = config
            .mcp
            .api_token
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty());
        let has_oauth = auth.mode == AuthMode::OAuth;
        if has_oauth && !has_static_token {
            return Err(anyhow::anyhow!(
                "MCP host `{}` is not a loopback address and CORTEX_AUTH_MODE=oauth is \
                 configured without CORTEX_TOKEN. OTLP /v1/logs only supports the static \
                 Bearer token gate today, so this would expose unauthenticated OTLP writes. \
                 Set CORTEX_TOKEN, bind to 127.0.0.1 / ::1, or enable an upstream auth \
                 gateway with CORTEX_NO_AUTH=true and CORTEX_TRUSTED_GATEWAY_NO_AUTH=true.",
                config.mcp.host
            ));
        }
        if !has_static_token && !has_oauth {
            return Err(anyhow::anyhow!(
                "MCP host `{}` is not a loopback address but no authentication is configured — \
                 set CORTEX_TOKEN, set CORTEX_AUTH_MODE=oauth, or bind to 127.0.0.1 / ::1",
                config.mcp.host
            ));
        }
    }

    Ok(())
}

pub fn mcp_bind_is_loopback(config: &Config) -> bool {
    IpAddr::from_str(&config.mcp.host)
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Returns true when the MCP HTTP listener is bound to a non-loopback address
/// AND the operator-facing public URL is plain http:// (or absent). In that
/// case `CORTEX_API_TOKEN` traverses the wire in plaintext. SWAG TLS
/// termination is the operator's responsibility — this helper just makes the
/// trade-off observable at startup.
pub fn api_token_plaintext_exposure(config: &Config) -> bool {
    if mcp_bind_is_loopback(config) {
        return false;
    }
    !matches!(
        config.mcp.auth.public_url.as_deref(),
        Some(url) if url.trim_start().to_ascii_lowercase().starts_with("https://"),
    )
}

fn option_is_blank(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|v| v.trim().is_empty())
}

pub(crate) fn validate_docker_ingest_config(config: &DockerIngestConfig) -> anyhow::Result<()> {
    if !config.enabled {
        return Ok(());
    }
    if config.hosts.is_empty() {
        return Err(anyhow::anyhow!(
            "docker_ingest.hosts must not be empty when docker ingest is enabled"
        ));
    }
    if config.reconnect_initial_ms == 0 {
        return Err(anyhow::anyhow!(
            "docker_ingest.reconnect_initial_ms must be > 0"
        ));
    }
    if config.reconnect_max_ms < config.reconnect_initial_ms {
        return Err(anyhow::anyhow!(
            "docker_ingest.reconnect_max_ms must be >= reconnect_initial_ms"
        ));
    }
    let mut names = HashSet::new();
    for host in &config.hosts {
        if host.name.trim().is_empty() {
            return Err(anyhow::anyhow!("docker_ingest host name must not be empty"));
        }
        if !names.insert(host.name.as_str()) {
            return Err(anyhow::anyhow!(
                "duplicate docker_ingest host name: {}",
                host.name
            ));
        }
        if !(host.base_url.starts_with("http://") || host.base_url.starts_with("https://")) {
            return Err(anyhow::anyhow!(
                "docker_ingest host {} base_url must start with http:// or https://",
                host.name
            ));
        }
        if host.base_url.starts_with("http://") && !host.allow_insecure_http {
            return Err(anyhow::anyhow!(
                "docker_ingest host {} uses insecure http://; set allow_insecure_http = true only for trusted private networks",
                host.name
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_receiver_config(config: &ReceiverConfig) -> anyhow::Result<()> {
    if config.max_message_size == 0 {
        return Err(anyhow::anyhow!("syslog.max_message_size must be > 0"));
    }
    if config.max_tcp_connections == 0 {
        return Err(anyhow::anyhow!("syslog.max_tcp_connections must be > 0"));
    }
    if config.tcp_idle_timeout_secs == 0 {
        return Err(anyhow::anyhow!("syslog.tcp_idle_timeout_secs must be > 0"));
    }
    if config.batch_size == 0 {
        return Err(anyhow::anyhow!("syslog.batch_size must be > 0"));
    }
    if config.flush_interval == 0 {
        return Err(anyhow::anyhow!("syslog.flush_interval must be > 0"));
    }
    if config.write_channel_capacity == 0 {
        return Err(anyhow::anyhow!("syslog.write_channel_capacity must be > 0"));
    }
    for cidr in &config.allowed_source_cidrs {
        if let Some((prefix, len)) = cidr.split_once('/') {
            let prefix_ok = prefix.parse::<std::net::IpAddr>().is_ok();
            let len_ok = len.parse::<u32>().is_ok();
            if !prefix_ok || !len_ok {
                return Err(anyhow::anyhow!(
                    "CORTEX_ALLOWED_SOURCE_CIDRS: invalid CIDR entry '{cidr}' — \
                     expected format is <ip>/<prefix_len> (e.g. 10.0.0.0/8)"
                ));
            }
        } else {
            return Err(anyhow::anyhow!(
                "CORTEX_ALLOWED_SOURCE_CIDRS: invalid entry '{cidr}' — \
                 missing prefix length (e.g. 10.0.0.0/8)"
            ));
        }
    }
    Ok(())
}

/// Returns `true` only when the token is `Some` but contains only whitespace.
/// Returns `false` for `None` — an absent token is not an error; callers that
/// need to enforce token presence must check for `None` separately.
fn token_is_set_but_blank(token: &Option<String>) -> bool {
    token
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
}

fn env_override_parse<T: std::str::FromStr>(key: &str, target: &mut T) -> anyhow::Result<()>
where
    T::Err: std::fmt::Display,
{
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            *target = v
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid value for {key}={v}: {e}"))?;
        }
    }
    Ok(())
}

fn validate_host(host: &str) -> anyhow::Result<()> {
    // Accept IP addresses and hostnames. A quick parse check — if it's an IP, validate it.
    // Hostnames are validated at bind time by Tokio.
    if host.contains(':') {
        return Err(anyhow::anyhow!(
            "Host '{host}' should not contain a port — use the separate port setting"
        ));
    }
    Ok(())
}

fn validate_storage_config(storage: &StorageConfig) -> anyhow::Result<()> {
    if storage.max_db_size_mb > 0 {
        if storage.recovery_db_size_mb == 0 {
            return Err(anyhow::anyhow!(
                "recovery_db_size_mb must be > 0 when max_db_size_mb is enabled"
            ));
        }
        if storage.recovery_db_size_mb >= storage.max_db_size_mb {
            return Err(anyhow::anyhow!(
                "recovery_db_size_mb must be lower than max_db_size_mb"
            ));
        }
    } else if storage.recovery_db_size_mb != 0 {
        return Err(anyhow::anyhow!(
            "recovery_db_size_mb must be 0 when max_db_size_mb is disabled"
        ));
    }

    if storage.min_free_disk_mb > 0 {
        if storage.recovery_free_disk_mb == 0 {
            return Err(anyhow::anyhow!(
                "recovery_free_disk_mb must be > 0 when min_free_disk_mb is enabled"
            ));
        }
        if storage.recovery_free_disk_mb <= storage.min_free_disk_mb {
            return Err(anyhow::anyhow!(
                "recovery_free_disk_mb must be higher than min_free_disk_mb"
            ));
        }
    } else if storage.recovery_free_disk_mb != 0 {
        return Err(anyhow::anyhow!(
            "recovery_free_disk_mb must be 0 when min_free_disk_mb is disabled"
        ));
    }

    if storage.cleanup_interval_secs < 5 {
        return Err(anyhow::anyhow!(
            "cleanup_interval_secs must be at least 5 seconds"
        ));
    }

    if storage.cleanup_chunk_size == 0 {
        return Err(anyhow::anyhow!("cleanup_chunk_size must be > 0"));
    }

    if storage.cleanup_chunk_size > MAX_CLEANUP_CHUNK_SIZE {
        return Err(anyhow::anyhow!(
            "cleanup_chunk_size must be <= {} (larger values hold the write lock too long)",
            MAX_CLEANUP_CHUNK_SIZE
        ));
    }

    // err+ retention floor (syslog-mcp-w4hh). The floor is dimensioned in
    // (time window × per-source row count), NOT bytes, so there is no
    // meaningful "floor < max_db_size_mb" byte comparison. The coherent
    // invariant is that the floor must not be self-contradictory: if a window
    // is configured, the per-source cap must be > 0, otherwise the floor would
    // protect a non-empty time window yet retain zero rows from it — a silent
    // footgun that re-enables the err+ self-wipe the floor exists to prevent.
    if storage.err_floor_window_hours > 0 && storage.err_floor_per_source_cap == 0 {
        return Err(anyhow::anyhow!(
            "err_floor_per_source_cap must be > 0 when err_floor_window_hours is set \
             (a window with a zero per-source cap protects no err+ rows)"
        ));
    }

    Ok(())
}

/// Validate the error detection configuration.
fn validate_error_detection_config(cfg: &ErrorDetectionConfig) -> anyhow::Result<()> {
    if cfg.scan_interval_secs == 0 {
        anyhow::bail!("[error_detection] scan_interval_secs must be > 0");
    }
    Ok(())
}

/// Validate the notifications configuration.
///
/// Fails at startup if notifications are enabled but no Apprise URLs are
/// configured — without URLs all notifications would be silently dropped.
fn validate_notifications_config(cfg: &NotificationsConfig) -> anyhow::Result<()> {
    if cfg.dispatcher_interval_secs == 0 {
        anyhow::bail!("[notifications] dispatcher_interval_secs must be > 0");
    }
    if cfg.evaluators.evaluator_interval_secs == 0 {
        anyhow::bail!("[notifications] evaluator_interval_secs must be > 0");
    }
    if cfg.evaluators.ingest_silence && cfg.evaluators.ingest_silence_threshold_secs == 0 {
        anyhow::bail!(
            "[notifications] ingest_silence_threshold_secs must be > 0 when \
             ingest_silence is enabled"
        );
    }
    // Trim whitespace before checking emptiness to catch " " entries.
    let has_apprise_url = !cfg.apprise_url.trim().is_empty();
    let has_apprise_urls = cfg.apprise_urls.iter().any(|u| !u.trim().is_empty());
    if cfg.enabled && !has_apprise_url && !has_apprise_urls {
        anyhow::bail!(
            "[notifications] enabled = true but no apprise_urls configured; \
             all notifications will be silently dropped. \
             Configure apprise_urls (or apprise_url) or set enabled = false."
        );
    }
    Ok(())
}

#[cfg(test)]
impl StorageConfig {
    /// Returns a minimal StorageConfig for use in unit tests.
    pub(crate) fn for_test(db_path: std::path::PathBuf) -> Self {
        Self {
            db_path,
            pool_size: 1,
            retention_days: 90,
            wal_mode: false,
            max_db_size_mb: 1024,
            recovery_db_size_mb: 900,
            min_free_disk_mb: 0,
            recovery_free_disk_mb: 0,
            cleanup_interval_secs: 60,
            cleanup_chunk_size: 1,
            err_floor_window_hours: default_err_floor_window_hours(),
            err_floor_per_source_cap: default_err_floor_per_source_cap(),
        }
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
