//! Time formatters and text-truncation helpers for CLI output.
#![allow(dead_code)]

/// Zero-copy char-boundary-safe truncation. Returns a `&str` slice of at most
/// `max_chars` Unicode scalar values.
pub(crate) fn truncate_chars(s: &str, max_chars: usize) -> &str {
    s.char_indices().nth(max_chars).map_or(s, |(i, _)| &s[..i])
}

/// Truncate to `max_chars`, appending `…` when truncated. The ellipsis counts
/// toward `max_chars`, so the visible output never exceeds the cap.
pub(crate) fn truncate_display_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    format!("{}…", truncate_chars(text, max_chars - 1))
}

/// Human-readable elapsed duration: "3s", "12m30s", "1h45m", "2d3h".
pub(crate) fn format_duration(mut secs: u64) -> String {
    let days = secs / 86_400;
    secs %= 86_400;
    let hours = secs / 3_600;
    secs %= 3_600;
    let minutes = secs / 60;
    let seconds = secs % 60;

    if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m{seconds}s")
    } else {
        format!("{seconds}s")
    }
}

/// Human-readable relative age from a UTC RFC 3339 timestamp string.
/// Returns "3s ago", "12m ago", "2h ago", "4d ago".
pub(crate) fn format_age(utc_rfc3339: &str) -> String {
    use chrono::{DateTime, Utc};
    let Ok(dt) = DateTime::parse_from_rfc3339(utc_rfc3339) else {
        return utc_rfc3339.to_string();
    };
    let secs = (Utc::now() - dt.with_timezone(&Utc)).num_seconds().max(0) as u64;
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;
