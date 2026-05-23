use anyhow::{bail, Result};
use syslog_mcp::app::{IncidentResponse, ServiceLogsResponse};
use syslog_mcp::scanner::{
    AiDoctorReport, CheckpointEntry, IndexResult, ParseErrorEntry, PruneCheckpointsResult,
};

use super::ai_watch::{AiSmokeWatchReport, AiWatchStatusReport};
use super::output_common::{local_ts, print_json, truncate};
pub(crate) fn print_checkpoints_response(response: &[CheckpointEntry], json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{} checkpoint(s)", response.len());
    println!(
        "{:<12} {:<8} {:<6} {:<7} {:<7} PATH",
        "KIND", "RECORDS", "PARSE", "MISSING", "ERROR"
    );
    for checkpoint in response {
        println!(
            "{:<12} {:<8} {:<6} {:<7} {:<7} {}",
            checkpoint.source_kind,
            checkpoint.imported_records,
            checkpoint.parse_errors,
            if checkpoint.missing { "yes" } else { "-" },
            if checkpoint.last_error.is_some() {
                "yes"
            } else {
                "-"
            },
            truncate(&checkpoint.canonical_path, 80),
        );
        if let Some(error) = &checkpoint.last_error {
            println!("    error: {}", truncate(error, 160));
        }
    }
    Ok(())
}

pub(crate) fn print_ai_parse_errors_response(
    response: &[ParseErrorEntry],
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{} parse error(s)", response.len());
    println!(
        "{:<24} {:<8} {:<8} {:<40} ERROR",
        "SEEN", "KIND", "LINE", "PATH"
    );
    for error in response {
        println!(
            "{:<24} {:<8} {:<8} {:<40} {}",
            truncate(&error.seen_at, 23),
            truncate(&error.source_kind, 8),
            error.line_no,
            truncate(&error.canonical_path, 39),
            truncate(&error.error, 100),
        );
        if let Some(preview) = &error.record_preview {
            println!("    preview: {}", truncate(preview, 160));
        }
    }
    Ok(())
}

pub(crate) fn print_prune_checkpoints_response(
    response: &PruneCheckpointsResult,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "matched={} pruned={} dry_run={}",
        response.matched, response.pruned, response.dry_run
    );
    for path in &response.paths {
        println!("  {}", path);
    }
    Ok(())
}

pub(crate) fn print_ai_doctor_response(response: &AiDoctorReport, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("db_path: {}", response.db_path);
    println!(
        "db_schema_version: {}/{} ({})",
        response.db_schema_version,
        response.known_schema_version,
        if response.schema_current {
            "current"
        } else {
            "behind"
        }
    );
    println!(
        "db_last_migration_at: {}",
        response.db_last_migration_at.as_deref().unwrap_or("-")
    );
    println!(
        "claude_root: {} ({}, readable={}, writable={}, owner={:?}:{:?}, mode={:?}, strict_ok={})",
        response.claude_root.path,
        if response.claude_root.exists {
            "exists"
        } else {
            "missing"
        },
        response.claude_root.readable,
        response.claude_root.writable,
        response.claude_root.owner_uid,
        response.claude_root.owner_gid,
        response.claude_root.mode.map(|mode| format!("{mode:o}")),
        response.claude_root.strict_ok
    );
    println!(
        "codex_root: {} ({}, readable={}, writable={}, owner={:?}:{:?}, mode={:?}, strict_ok={})",
        response.codex_root.path,
        if response.codex_root.exists {
            "exists"
        } else {
            "missing"
        },
        response.codex_root.readable,
        response.codex_root.writable,
        response.codex_root.owner_uid,
        response.codex_root.owner_gid,
        response.codex_root.mode.map(|mode| format!("{mode:o}")),
        response.codex_root.strict_ok
    );
    println!("checkpoint_count: {}", response.checkpoint_count);
    println!(
        "checkpoint_error_count: {}",
        response.checkpoint_error_count
    );
    println!(
        "missing_checkpoint_count: {}",
        response.missing_checkpoint_count
    );
    println!("imported_record_count: {}", response.imported_record_count);
    println!("parse_error_count: {}", response.parse_error_count);
    println!(
        "newest_indexed: {} {}",
        response.newest_indexed_at.as_deref().unwrap_or("-"),
        response.newest_indexed_path.as_deref().unwrap_or("-")
    );
    Ok(())
}

pub(crate) fn print_ai_watch_status_response(
    response: &AiWatchStatusReport,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("service: {}", response.service);
    println!("active: {}", response.active.as_deref().unwrap_or("-"));
    println!("enabled: {}", response.enabled.as_deref().unwrap_or("-"));
    println!(
        "main_pid: {}",
        response
            .main_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "exec_start: {}",
        response.exec_start.as_deref().unwrap_or("-")
    );
    println!(
        "process_start_time: {}",
        response.process_start_time.as_deref().unwrap_or("-")
    );
    println!("db_path: {}", response.db_path);
    println!(
        "db_schema_version: {}/{}",
        response.health.db_schema_version, response.health.known_schema_version
    );
    println!(
        "schema_drift_detected: {}",
        response.health.schema_drift_detected
    );
    println!(
        "last_successful_ingest_at: {}",
        response
            .health
            .last_successful_ingest_at
            .as_deref()
            .unwrap_or("-")
    );
    println!(
        "recent_failure_count: {}",
        response.health.recent_failure_count
    );
    if !response.health.stale_indicators.is_empty() {
        println!(
            "stale_indicators: {}",
            response.health.stale_indicators.join(", ")
        );
    }
    if !response.latest_journal.is_empty() {
        println!("latest_journal:");
        for line in &response.latest_journal {
            println!("  {line}");
        }
    }
    Ok(())
}

pub(crate) fn print_service_logs_response(report: &ServiceLogsResponse, json: bool) -> Result<()> {
    if json {
        return print_json(report);
    }
    if report.dropped_lines > 0 {
        eprintln!(
            "warning: {} malformed journal line(s) dropped",
            report.dropped_lines
        );
    }
    if report.entries.is_empty() {
        println!("{}: 0 journal entries", report.service);
        return Ok(());
    }
    for entry in &report.entries {
        let timestamp = entry.timestamp.as_deref().unwrap_or("-");
        let ident = entry
            .syslog_identifier
            .as_deref()
            .or(entry.unit.as_deref())
            .unwrap_or("-");
        let message = entry.message.as_deref().unwrap_or("");
        println!("{timestamp} {ident}: {message}");
    }
    Ok(())
}

pub(crate) fn print_incident_response(response: &IncidentResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "incident around {} +/- {}m: {} event(s){}",
        response.around,
        response.window_minutes,
        response.event_count,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for warning in &response.warnings {
        println!("warn: {warning}");
    }
    for event in &response.events {
        let host = event.host.as_deref().unwrap_or("-");
        let severity = event.severity.as_deref().unwrap_or("-");
        let app = event.app.as_deref().unwrap_or("-");
        println!(
            "{} {} {} {} {}: {}",
            local_ts(&event.timestamp),
            event.source,
            host,
            severity,
            app,
            event.message
        );
    }
    Ok(())
}

pub(crate) fn print_ai_smoke_watch_response(
    response: &AiSmokeWatchReport,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("session_id: {}", response.session_id);
    println!("transcript_path: {}", response.transcript_path.display());
    println!("ingested: {}", response.ingested);
    println!(
        "pruned_missing_checkpoint: {}",
        response.pruned_missing_checkpoint
    );
    println!(
        "missing_checkpoint_count: {}",
        response.missing_checkpoint_count
    );
    Ok(())
}

pub(crate) fn print_index_response(response: &IndexResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "files={} ingested={} duplicates={} parse_errors={} skipped={} unsupported={} symlinks={} unsafe_paths={} storage_blocked_chunks={} dropped_metadata_fields={} checkpoint_updates={} file_errors={}",
        response.discovered_files,
        response.ingested,
        response.skipped_dupes,
        response.parse_errors,
        response.skipped_files,
        response.unsupported_files,
        response.skipped_symlinks,
        response.skipped_unsafe_paths,
        response.storage_blocked_chunks,
        response.dropped_metadata_fields,
        response.checkpoint_updates,
        response.file_errors.len()
    );
    for error in &response.file_errors {
        eprintln!("index error: {}: {}", error.path, error.error);
    }
    Ok(())
}

pub(crate) fn ensure_index_success(response: &IndexResult) -> Result<()> {
    if response.file_errors.is_empty()
        && response.storage_blocked_chunks == 0
        && response.parse_errors == 0
    {
        if response.dropped_metadata_fields > 0 {
            eprintln!(
                "warning: {} transcript metadata field(s) were dropped",
                response.dropped_metadata_fields
            );
        }
        Ok(())
    } else if response.storage_blocked_chunks > 0 {
        bail!(
            "{} transcript chunk(s) blocked by storage guardrails",
            response.storage_blocked_chunks
        )
    } else if response.parse_errors > 0 {
        bail!(
            "{} transcript record(s) failed to parse",
            response.parse_errors
        )
    } else {
        bail!(
            "{} transcript file(s) failed to index",
            response.file_errors.len()
        )
    }
}

pub(crate) fn ensure_ai_doctor_success(
    response: &AiDoctorReport,
    strict_permissions: bool,
) -> Result<()> {
    if strict_permissions
        && ((response.claude_root.exists && !response.claude_root.strict_ok)
            || (response.codex_root.exists && !response.codex_root.strict_ok))
    {
        bail!("AI transcript root permission check failed");
    }
    Ok(())
}

#[cfg(test)]
#[path = "output_ai_tests.rs"]
mod tests;
