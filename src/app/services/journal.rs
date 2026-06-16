const CORTEX_OWNED_USER_SERVICES: &[&str] = &[
    "cortex-ai-watch.service",
    "cortex-ai-index.service",
    "cortex.service",
];

pub(super) fn normalize_syslog_owned_service(service: &str) -> ServiceResult<String> {
    let unit = if service.ends_with(".service") {
        service.to_string()
    } else {
        format!("{service}.service")
    };
    if CORTEX_OWNED_USER_SERVICES.contains(&unit.as_str()) {
        Ok(unit)
    } else {
        Err(ServiceError::InvalidInput(format!(
            "unsupported cortex-owned service '{service}'; expected one of {}",
            CORTEX_OWNED_USER_SERVICES.join(", ")
        )))
    }
}

// `command_output`, `inferred_user_bus_env`, and `current_uid` were extracted
// to `os_adapter.rs` as part of Arch-C2. OS-level shell-outs now go through
// the `OsAdapter` trait so they can be injected in tests.

/// Parse journalctl `-o json` output into entries, tolerating malformed lines.
///
/// Returns `(entries, dropped)` so callers can surface a warning when the
/// journal contains corrupt rows — `service logs` is a self-debugging surface
/// and must not nuke a 5000-line response because one line failed to parse.
pub(super) fn parse_journal_json_lines(raw: &str) -> (Vec<ServiceJournalEntry>, usize) {
    let mut entries = Vec::new();
    let mut dropped: usize = 0;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        match parse_journal_json_line(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => dropped = dropped.saturating_add(1),
        }
    }
    (entries, dropped)
}

fn parse_journal_json_line(line: &str) -> ServiceResult<ServiceJournalEntry> {
    let value: serde_json::Value = serde_json::from_str(line).map_err(anyhow::Error::from)?;
    Ok(ServiceJournalEntry {
        timestamp: journal_string(&value, "__REALTIME_TIMESTAMP")
            .and_then(|micros| journal_realtime_timestamp(&micros)),
        realtime_timestamp_us: journal_string(&value, "__REALTIME_TIMESTAMP"),
        unit: journal_string(&value, "_SYSTEMD_USER_UNIT")
            .or_else(|| journal_string(&value, "_SYSTEMD_UNIT")),
        priority: journal_string(&value, "PRIORITY"),
        syslog_identifier: journal_string(&value, "CORTEX_IDENTIFIER"),
        pid: journal_string(&value, "_PID"),
        message: journal_string(&value, "MESSAGE"),
        cursor: journal_string(&value, "__CURSOR"),
    })
}

fn journal_string(value: &serde_json::Value, key: &str) -> Option<String> {
    match value.get(key)? {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Array(values) => values.iter().find_map(|value| match value {
            serde_json::Value::String(value) => Some(value.clone()),
            _ => None,
        }),
        other => Some(other.to_string()),
    }
}

fn journal_realtime_timestamp(micros: &str) -> Option<String> {
    let micros = micros.parse::<i64>().ok()?;
    let secs = micros.div_euclid(1_000_000);
    let nanos = micros.rem_euclid(1_000_000) as u32 * 1_000;
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos).map(time::rfc3339_z)
}

/// callers can invoke it without standing up a [`CortexService`] (and the
/// SQLite pool that backs it) — `cortex service logs` is a self-debugging
/// surface that must work when the DB is corrupted, locked, or full.
///
/// The `os` parameter is the `OsAdapter` to use for the journalctl shell-out.
/// Pass `&SystemOsAdapter` for production; inject a mock for tests.
pub async fn run_service_logs(
    req: ServiceLogsRequest,
    os: &(dyn os_adapter::OsAdapter + Send + Sync),
) -> ServiceResult<ServiceLogsResponse> {
    let service = normalize_syslog_owned_service(&req.service)?;
    let mut args = vec![
        "--user".to_string(),
        "-u".to_string(),
        service.clone(),
        "--no-pager".to_string(),
        "--output".to_string(),
        "json".to_string(),
    ];
    if let Some(from) = &req.since {
        // Validate as RFC 3339 before passing to journalctl to prevent
        // argument injection (e.g. "--rotate", "--vacuum-size=1").
        chrono::DateTime::parse_from_rfc3339(from)
            .map_err(|_| ServiceError::InvalidInput(format!("invalid `from` timestamp: {from}")))?;
        args.push("--since".to_string());
        args.push(from.clone());
    }
    if let Some(to) = &req.until {
        chrono::DateTime::parse_from_rfc3339(to)
            .map_err(|_| ServiceError::InvalidInput(format!("invalid `to` timestamp: {to}")))?;
        args.push("--until".to_string());
        args.push(to.clone());
    }
    let tail = req.tail.map(|tail| tail.clamp(1, 5_000));
    if let Some(tail) = tail {
        args.push("-n".to_string());
        args.push(tail.to_string());
    }

    let raw = os.run_command("journalctl", &args).await?;
    let (entries, dropped_lines) = parse_journal_json_lines(&raw);
    if dropped_lines > 0 {
        tracing::warn!(
            service = %service,
            dropped_lines,
            "service_logs: skipped malformed journal lines"
        );
    }
    Ok(ServiceLogsResponse {
        service,
        from: req.since,
        to: req.until,
        tail,
        entries,
        dropped_lines,
    })
}
use super::*;

impl CortexService {
    pub async fn service_logs(
        &self,
        req: ServiceLogsRequest,
    ) -> ServiceResult<ServiceLogsResponse> {
        run_service_logs(req, self.os.as_ref()).await
    }
}
