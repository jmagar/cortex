use chrono::{DateTime, SecondsFormat, Utc};

use super::{ServiceError, ServiceResult};

/// Format a UTC instant in the same `Z`-suffixed shape SQLite stores in
/// `received_at` (and that OTLP ingest writes for `timestamp`), so
/// lexicographic TEXT comparisons line up at boundary instants. Plain
/// `to_rfc3339()` produces the `+00:00` form, which sorts strictly less than
/// `Z` character-by-character and can silently drop equal-instant rows from
/// boundary windows.
pub(crate) fn rfc3339_z(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn parse_optional_timestamp(
    raw: Option<&str>,
    field_name: &str,
) -> ServiceResult<Option<String>> {
    raw.map(|s| parse_required_timestamp(s, field_name).map(rfc3339_z))
        .transpose()
}

pub(super) fn parse_required_timestamp(
    raw: &str,
    field_name: &str,
) -> ServiceResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map_err(|e| {
            ServiceError::InvalidInput(format!(
                "Invalid {field_name} '{}': {e}. Expected ISO 8601 / RFC3339 format, e.g. '2025-01-15T00:00:00Z'",
                raw
            ))
        })
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
#[path = "time_tests.rs"]
mod tests;
