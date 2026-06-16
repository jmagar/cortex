//! Normalize user-supplied time arguments to RFC3339.
//!
//! Accepts relative durations (`1h`, `30m`, `2d`, `90s`), the keywords
//! `now`/`today`/`yesterday`, and absolute timestamps (RFC3339, `YYYY-MM-DD`,
//! `YYYY-MM-DD HH:MM`). `now` is injected for deterministic testing — never
//! read the clock inside this module.

use anyhow::{Result, bail};
use chrono::{DateTime, Duration, TimeZone, Utc};

/// Convert a user time string into an RFC3339 timestamp string.
pub(crate) fn parse_time_arg(input: &str, now: DateTime<Utc>) -> Result<String> {
    let s = input.trim();
    if s.is_empty() {
        bail!("empty time value");
    }
    match s.to_ascii_lowercase().as_str() {
        "now" => return Ok(now.to_rfc3339()),
        "today" => return Ok(start_of_day(now, 0).to_rfc3339()),
        "yesterday" => return Ok(start_of_day(now, 1).to_rfc3339()),
        _ => {}
    }
    if let Some(dt) = parse_relative(s, now)? {
        return Ok(dt.to_rfc3339());
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc).to_rfc3339());
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(Utc
            .from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
            .to_rfc3339());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
        return Ok(Utc.from_utc_datetime(&ndt).to_rfc3339());
    }
    bail!(
        "unrecognized time value '{s}'; use e.g. 1h, 30m, 2d, yesterday, 2026-06-01, or an RFC3339 timestamp"
    )
}

/// Parse a relative duration (`<int><unit>`). Returns `Ok(None)` when `s` is not
/// a relative form so the caller can try absolute parsing; errors only when the
/// numeric prefix parses but the unit is unknown.
fn parse_relative(s: &str, now: DateTime<Utc>) -> Result<Option<DateTime<Utc>>> {
    let (value, unit) = s.split_at(s.len() - 1);
    let unit_char = s.chars().last().unwrap();
    if let Ok(n) = value.parse::<i64>() {
        let dur = match unit_char {
            's' => Duration::seconds(n),
            'm' => Duration::minutes(n),
            'h' => Duration::hours(n),
            'd' => Duration::days(n),
            _ => bail!("unknown time unit '{unit}'; use s, m, h, or d (e.g. 90s, 2d)"),
        };
        return Ok(Some(now - dur));
    }
    Ok(None)
}

/// Midnight UTC, `days_ago` days before `now`.
fn start_of_day(now: DateTime<Utc>, days_ago: i64) -> DateTime<Utc> {
    let d = (now - Duration::days(days_ago)).date_naive();
    Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
}

#[cfg(test)]
#[path = "timearg_tests.rs"]
mod tests;
