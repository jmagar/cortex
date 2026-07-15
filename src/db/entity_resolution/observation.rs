//! Bounded typed resolver observations.
//!
//! Observations are chunk-local, in-memory inputs to the deterministic
//! resolver. They are never persisted per-log-row; projection code converts
//! source rows into observations, resolves them, and stores only the
//! resulting graph entities/relationships/evidence.

/// Epistemic trust of an observation's source. Ordered strongest-first so
/// `min()` over evidence selects the strongest supporting evidence:
/// independent corroboration cannot be weakened by additional weak
/// observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResolverTrust {
    Verified,
    Claimed,
    Inferred,
}

/// What kind of thing an observation describes.
// Plan-locked vocabulary (2026-07-13 canonical-entity-resolution): several
// kinds are reserved for future adapters and not constructed yet.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationKind {
    Host,
    LogicalService,
    ServiceInstance,
    Container,
    ComposeProject,
    Domain,
    ReverseProxy,
    Storage,
    ConfigArtifact,
    RawAppLabel,
    AiProject,
    AiSession,
    Command,
    User,
    Device,
}

/// One bounded, typed observation extracted from a source row. Display
/// values must already be safe (see [`safe_display_value`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolverObservation {
    pub kind: ObservationKind,
    pub observed_key: String,
    pub display_label: String,
    pub host_key: Option<String>,
    pub logical_service_key: Option<String>,
    pub service_instance_key: Option<String>,
    pub source_kind: String,
    pub source_id: String,
    pub evidence_path: String,
    pub observed_at: String,
    pub trust: ResolverTrust,
    pub structured: bool,
}

/// Structured agent-attested Docker identity for one log line, extracted
/// from `metadata_json.agent_docker`. This is the supported Docker identity
/// source; central-pull `docker://` rows are not resolver proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDockerIdentity {
    pub agent_host: String,
    pub container_id: String,
    pub container_name: String,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
    pub image: Option<String>,
    pub stream: String,
    pub observed_at: String,
}

/// Redact display values that look sensitive (credentialed URLs, home paths,
/// token/secret material, metadata payload paths) and bound the rest to 128
/// printable characters.
pub fn safe_display_value(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    // `:` + `@` catches scheme-less credentials (`user:pass@host`) as well
    // as credentialed URLs; over-redaction is acceptable here.
    let sensitive = lower.contains(':') && lower.contains('@')
        || lower.contains("token")
        || lower.contains("password")
        || lower.contains("secret")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("/home/")
        || lower.contains("/users/")
        || lower.contains("metadata_json")
        || lower.contains("cache_path")
        || lower.contains("source_path");
    if sensitive {
        return "[redacted]".to_string();
    }
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(128)
        .collect()
}
