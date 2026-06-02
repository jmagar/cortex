#![allow(dead_code)]

//! Derived investigation graph schema vocabulary.
//!
//! The graph is a rebuildable projection over authoritative source tables
//! (`logs`, heartbeats, AI session rollups, source inventory, signatures). Keep
//! vocabulary constants here so schema, extraction, service, adapters, and docs
//! do not drift into hand-written string variants.

pub const ENTITY_TYPE_HOST: &str = "host";
pub const ENTITY_TYPE_CONTAINER: &str = "container";
pub const ENTITY_TYPE_SERVICE: &str = "service";
pub const ENTITY_TYPE_APP: &str = "app";
pub const ENTITY_TYPE_SOURCE_IP: &str = "source_ip";
pub const ENTITY_TYPE_AI_PROJECT: &str = "ai_project";
pub const ENTITY_TYPE_AI_SESSION: &str = "ai_session";
pub const ENTITY_TYPE_ERROR_SIGNATURE: &str = "error_signature";

pub const ENTITY_TYPES: &[&str] = &[
    ENTITY_TYPE_HOST,
    ENTITY_TYPE_CONTAINER,
    ENTITY_TYPE_SERVICE,
    ENTITY_TYPE_APP,
    ENTITY_TYPE_SOURCE_IP,
    ENTITY_TYPE_AI_PROJECT,
    ENTITY_TYPE_AI_SESSION,
    ENTITY_TYPE_ERROR_SIGNATURE,
];

pub const REL_OBSERVED_AS: &str = "observed_as";
pub const REL_RUNS_ON: &str = "runs_on";
pub const REL_EMITTED_BY: &str = "emitted_by";
pub const REL_WORKED_ON: &str = "worked_on";
pub const REL_MATCHES_SIGNATURE: &str = "matches_signature";

pub const RELATIONSHIP_TYPES: &[&str] = &[
    REL_OBSERVED_AS,
    REL_RUNS_ON,
    REL_EMITTED_BY,
    REL_WORKED_ON,
    REL_MATCHES_SIGNATURE,
];

pub const TRUST_VERIFIED: &str = "verified";
pub const TRUST_CLAIMED: &str = "claimed";
pub const TRUST_INFERRED: &str = "inferred";
pub const TRUST_CORRELATED: &str = "correlated";

pub const TRUST_LEVELS: &[&str] = &[
    TRUST_VERIFIED,
    TRUST_CLAIMED,
    TRUST_INFERRED,
    TRUST_CORRELATED,
];

pub const SOURCE_KIND_LOG: &str = "log";
pub const SOURCE_KIND_HEARTBEAT: &str = "heartbeat";
pub const SOURCE_KIND_AI_SESSION_ROLLUP: &str = "ai_session_rollup";
pub const SOURCE_KIND_SOURCE_INVENTORY: &str = "source_inventory";
pub const SOURCE_KIND_APP_INVENTORY: &str = "app_inventory";
pub const SOURCE_KIND_ERROR_SIGNATURE: &str = "error_signature";

pub const EVIDENCE_SOURCE_KINDS: &[&str] = &[
    SOURCE_KIND_LOG,
    SOURCE_KIND_HEARTBEAT,
    SOURCE_KIND_AI_SESSION_ROLLUP,
    SOURCE_KIND_SOURCE_INVENTORY,
    SOURCE_KIND_APP_INVENTORY,
    SOURCE_KIND_ERROR_SIGNATURE,
];

pub const REASON_SYSLOG_CLAIMED_HOSTNAME: &str = "syslog_claimed_hostname";
pub const REASON_LOG_APP_NAME: &str = "log_app_name";
pub const REASON_DOCKER_CONTAINER_ID: &str = "docker_container_id";
pub const REASON_DOCKER_SERVICE_LABEL: &str = "docker_service_label";
pub const REASON_AI_SESSION_PROJECT: &str = "ai_session_project";
pub const REASON_HEARTBEAT_HOST_STATE: &str = "heartbeat_host_state";
pub const REASON_ERROR_SIGNATURE_MATCH: &str = "error_signature_match";

pub const REASON_CODES: &[&str] = &[
    REASON_SYSLOG_CLAIMED_HOSTNAME,
    REASON_LOG_APP_NAME,
    REASON_DOCKER_CONTAINER_ID,
    REASON_DOCKER_SERVICE_LABEL,
    REASON_AI_SESSION_PROJECT,
    REASON_HEARTBEAT_HOST_STATE,
    REASON_ERROR_SIGNATURE_MATCH,
];

pub const PROJECTION_STATUS_NEVER_BUILT: &str = "never_built";
pub const PROJECTION_STATUS_BUILDING: &str = "building";
pub const PROJECTION_STATUS_READY: &str = "ready";
pub const PROJECTION_STATUS_STALE: &str = "stale";
pub const PROJECTION_STATUS_FAILED: &str = "failed";

pub const PROJECTION_STATUSES: &[&str] = &[
    PROJECTION_STATUS_NEVER_BUILT,
    PROJECTION_STATUS_BUILDING,
    PROJECTION_STATUS_READY,
    PROJECTION_STATUS_STALE,
    PROJECTION_STATUS_FAILED,
];

pub fn is_known_entity_type(value: &str) -> bool {
    ENTITY_TYPES.contains(&value)
}

pub fn is_known_relationship_type(value: &str) -> bool {
    RELATIONSHIP_TYPES.contains(&value)
}

pub fn is_known_reason_code(value: &str) -> bool {
    REASON_CODES.contains(&value)
}

pub fn is_known_trust_level(value: &str) -> bool {
    TRUST_LEVELS.contains(&value)
}

pub fn is_known_evidence_source_kind(value: &str) -> bool {
    EVIDENCE_SOURCE_KINDS.contains(&value)
}
