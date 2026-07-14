//! Canonical entity/relationship/reason vocabulary and key grammar for the
//! resolver-backed graph projection contract.

pub const ENTITY_TYPE_LOGICAL_SERVICE: &str = "logical_service";
pub const ENTITY_TYPE_SERVICE_INSTANCE: &str = "service_instance";
pub const REL_INSTANCE_OF: &str = "instance_of";
pub const REASON_RESOLVER_INSTANCE_OF: &str = "resolver_instance_of";
pub const REASON_RESOLVER_SERVICE_INSTANCE: &str = "resolver_service_instance";
pub const REASON_RESOLVER_RAW_APP_LABEL: &str = "resolver_raw_app_label";
pub const GRAPH_PROJECTION_CONTRACT_KEY: &str = "graph_projection_contract";
pub const GRAPH_PROJECTION_CONTRACT_V2: &str = "entity_resolution_v2";

/// Legacy (pre entity-resolution) service identity shapes. These are
/// classified so callers can reject them; they are never normalized into
/// canonical keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyShape {
    HostService,
    HostProjectService,
    SlashTriplet,
}

/// Canonical logical-service key: lowercased, trimmed, non-key characters
/// mapped to `-`. Kept characters are ASCII alphanumerics plus `-`, `_`,
/// and `.` (dots preserve raw hostnames like `tootie.lan`). Returns `None`
/// when nothing canonical remains.
pub fn logical_service_key(name: &str) -> Option<String> {
    canonical_component(name)
}

/// Canonical service-instance key `host/service`. Host and service are each
/// canonicalized independently; `None` when either side is empty.
pub fn service_instance_key(host: &str, service: &str) -> Option<String> {
    Some(format!(
        "{}/{}",
        canonical_component(host)?,
        canonical_component(service)?
    ))
}

/// Split a canonical `host/service` key. Rejects empty components and any
/// extra `/` segments (which would be a legacy slash-triplet shape).
pub fn split_service_instance_key(key: &str) -> Option<(&str, &str)> {
    let (host, service) = key.split_once('/')?;
    if host.is_empty() || service.is_empty() || service.contains('/') {
        return None;
    }
    Some((host, service))
}

/// Classify legacy service identity shapes (`tootie:plex`,
/// `tootie:plex:plex`, `plex/plex/plex`). Canonical inputs return `None`, as
/// do free-text inputs that merely contain colons or slashes without looking
/// like legacy keys: anything with ASCII whitespace, colon shapes whose
/// segments are not all name-like (`10.0.0.5:443`, `12:30`), and
/// absolute paths (`/mnt/user/media`).
pub fn classify_legacy_shape(value: &str) -> Option<LegacyShape> {
    let trimmed = value.trim();
    if trimmed.chars().any(|ch| ch.is_ascii_whitespace()) {
        return None;
    }
    let colon_count = trimmed.matches(':').count();
    if colon_count >= 1 {
        let name_like_segments = trimmed
            .split(':')
            .all(|segment| segment.chars().any(|ch| ch.is_ascii_alphabetic()));
        if !name_like_segments {
            return None;
        }
        if colon_count == 1 {
            return Some(LegacyShape::HostService);
        }
        return Some(LegacyShape::HostProjectService);
    }
    let slash_count = trimmed.matches('/').count();
    if slash_count >= 2 && !trimmed.starts_with('/') {
        return Some(LegacyShape::SlashTriplet);
    }
    None
}

fn canonical_component(value: &str) -> Option<String> {
    let out = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(['-', '.'])
        .to_string();
    (!out.is_empty()).then_some(out)
}
