use anyhow::{bail, Result};
use syslog_mcp::app::{IncidentResponse, ServiceLogsResponse};
use syslog_mcp::scanner::{
    AiDoctorReport, CheckpointEntry, IndexResult, ParseErrorEntry, PruneCheckpointsResult,
};

use super::ai_watch::AiSmokeWatchReport;
use super::color::Palette;
use super::output_common::{local_ts, print_json, truncate};
use syslog_mcp::app::AiWatchStatusReport;

pub(crate) fn print_checkpoints_response(response: &[CheckpointEntry], json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!("{} checkpoint(s)", p.cyan(&response.len().to_string()));
    println!(
        "{}",
        p.muted(&format!(
            "{:<12} {:<8} {:<6} {:<7} {:<7} PATH",
            "KIND", "RECORDS", "PARSE", "MISSING", "ERROR"
        ))
    );
    for checkpoint in response {
        let has_error = checkpoint.last_error.is_some();
        println!(
            "{:<12} {:<8} {:<6} {:<7} {:<7} {}",
            p.violet(&checkpoint.source_kind),
            p.cyan(&checkpoint.imported_records.to_string()),
            if checkpoint.parse_errors > 0 {
                p.warn(&checkpoint.parse_errors.to_string()).to_string()
            } else {
                p.muted(&checkpoint.parse_errors.to_string()).to_string()
            },
            if checkpoint.missing {
                p.warn("yes").to_string()
            } else {
                p.muted("-").to_string()
            },
            if has_error {
                p.error("yes").to_string()
            } else {
                p.muted("-").to_string()
            },
            p.primary(&truncate(&checkpoint.canonical_path, 80)),
        );
        if let Some(error) = &checkpoint.last_error {
            println!(
                "    {}: {}",
                p.muted("error"),
                p.error(&truncate(error, 160))
            );
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
    let p = Palette::new();
    println!("{} parse error(s)", p.error(&response.len().to_string()));
    println!(
        "{}",
        p.muted(&format!(
            "{:<24} {:<8} {:<8} {:<40} ERROR",
            "SEEN", "KIND", "LINE", "PATH"
        ))
    );
    for error in response {
        println!(
            "{:<24} {:<8} {:<8} {:<40} {}",
            p.muted(&truncate(&error.seen_at, 23)),
            p.violet(&truncate(&error.source_kind, 8)),
            p.cyan(&error.line_no.to_string()),
            p.primary(&truncate(&error.canonical_path, 39)),
            p.error(&truncate(&error.error, 100)),
        );
        if let Some(preview) = &error.record_preview {
            println!("    {}: {}", p.muted("preview"), truncate(preview, 160));
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
    let p = Palette::new();
    println!(
        "matched={} pruned={} dry_run={}",
        p.cyan(&response.matched.to_string()),
        p.cyan(&response.pruned.to_string()),
        p.primary(&response.dry_run.to_string())
    );
    for path in &response.paths {
        println!("  {}", p.primary(path));
    }
    Ok(())
}

pub(crate) fn print_ai_doctor_response(response: &AiDoctorReport, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!("{}: {}", p.muted("db_path"), p.primary(&response.db_path));
    let schema_status = if response.schema_current {
        p.success("current").to_string()
    } else {
        p.warn("behind").to_string()
    };
    println!(
        "{}: {}/{} ({})",
        p.muted("db_schema_version"),
        p.cyan(&response.db_schema_version.to_string()),
        p.cyan(&response.known_schema_version.to_string()),
        schema_status
    );
    println!(
        "{}: {}",
        p.muted("db_last_migration_at"),
        p.primary(response.db_last_migration_at.as_deref().unwrap_or("-"))
    );
    let cr = &response.claude_root;
    println!(
        "{}: {} ({}, readable={}, writable={}, owner={:?}:{:?}, mode={:?}, strict_ok={})",
        p.muted("claude_root"),
        p.primary(&cr.path),
        if cr.exists {
            p.success("exists").to_string()
        } else {
            p.warn("missing").to_string()
        },
        cr.readable,
        cr.writable,
        cr.owner_uid,
        cr.owner_gid,
        cr.mode.map(|m| format!("{m:o}")),
        if cr.strict_ok {
            p.success("true").to_string()
        } else {
            p.warn("false").to_string()
        }
    );
    let xr = &response.codex_root;
    println!(
        "{}: {} ({}, readable={}, writable={}, owner={:?}:{:?}, mode={:?}, strict_ok={})",
        p.muted("codex_root"),
        p.primary(&xr.path),
        if xr.exists {
            p.success("exists").to_string()
        } else {
            p.warn("missing").to_string()
        },
        xr.readable,
        xr.writable,
        xr.owner_uid,
        xr.owner_gid,
        xr.mode.map(|m| format!("{m:o}")),
        if xr.strict_ok {
            p.success("true").to_string()
        } else {
            p.warn("false").to_string()
        }
    );
    println!(
        "{}: {}",
        p.muted("checkpoint_count"),
        p.cyan(&response.checkpoint_count.to_string())
    );
    println!(
        "{}: {}",
        p.muted("checkpoint_error_count"),
        if response.checkpoint_error_count > 0 {
            p.error(&response.checkpoint_error_count.to_string())
                .to_string()
        } else {
            p.cyan(&response.checkpoint_error_count.to_string())
                .to_string()
        }
    );
    println!(
        "{}: {}",
        p.muted("missing_checkpoint_count"),
        if response.missing_checkpoint_count > 0 {
            p.warn(&response.missing_checkpoint_count.to_string())
                .to_string()
        } else {
            p.cyan(&response.missing_checkpoint_count.to_string())
                .to_string()
        }
    );
    println!(
        "{}: {}",
        p.muted("imported_record_count"),
        p.cyan(&response.imported_record_count.to_string())
    );
    println!(
        "{}: {}",
        p.muted("parse_error_count"),
        if response.parse_error_count > 0 {
            p.error(&response.parse_error_count.to_string()).to_string()
        } else {
            p.cyan(&response.parse_error_count.to_string()).to_string()
        }
    );
    println!(
        "{}: {} {}",
        p.muted("newest_indexed"),
        p.muted(response.newest_indexed_at.as_deref().unwrap_or("-")),
        p.primary(response.newest_indexed_path.as_deref().unwrap_or("-"))
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
    let p = Palette::new();
    println!("{}: {}", p.muted("service"), p.primary(&response.service));
    println!(
        "{}: {}",
        p.muted("active"),
        p.primary(response.active.as_deref().unwrap_or("-"))
    );
    println!(
        "{}: {}",
        p.muted("enabled"),
        p.primary(response.enabled.as_deref().unwrap_or("-"))
    );
    println!(
        "{}: {}",
        p.muted("main_pid"),
        p.cyan(
            &response
                .main_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_string())
        )
    );
    println!(
        "{}: {}",
        p.muted("exec_start"),
        p.primary(response.exec_start.as_deref().unwrap_or("-"))
    );
    println!(
        "{}: {}",
        p.muted("exec_main_start_timestamp"),
        p.muted(response.exec_main_start_timestamp.as_deref().unwrap_or("-"))
    );
    println!(
        "{}: {}",
        p.muted("process_start_time"),
        p.muted(response.process_start_time.as_deref().unwrap_or("-"))
    );
    println!("{}: {}", p.muted("db_path"), p.primary(&response.db_path));
    match response.health.as_ref() {
        Some(h) => {
            println!(
                "{}: {}/{}",
                p.muted("db_schema_version"),
                p.cyan(&h.db_schema_version.to_string()),
                p.cyan(&h.known_schema_version.to_string())
            );
            let drift_str = h.schema_drift_detected.to_string();
            println!(
                "{}: {}",
                p.muted("schema_drift_detected"),
                if h.schema_drift_detected {
                    p.error(&drift_str).to_string()
                } else {
                    p.success(&drift_str).to_string()
                }
            );
            println!(
                "{}: {}",
                p.muted("last_successful_ingest_at"),
                p.muted(h.last_successful_ingest_at.as_deref().unwrap_or("-"))
            );
            println!(
                "{}: {}",
                p.muted("recent_failure_count"),
                if h.recent_failure_count > 0 {
                    p.error(&h.recent_failure_count.to_string()).to_string()
                } else {
                    p.cyan(&h.recent_failure_count.to_string()).to_string()
                }
            );
            if !h.stale_indicators.is_empty() {
                println!(
                    "{}: {}",
                    p.muted("stale_indicators"),
                    p.warn(&h.stale_indicators.join(", "))
                );
            }
        }
        None => {
            if let Some(err) = response.health_error.as_deref() {
                println!(
                    "{}: {}",
                    p.muted("health"),
                    p.warn(&format!("unavailable ({err})"))
                );
            } else {
                println!("{}: {}", p.muted("health"), p.warn("unavailable"));
            }
        }
    }
    if let Some(err) = response.journal_error.as_deref() {
        println!(
            "{}: {}",
            p.muted("latest_journal"),
            p.warn(&format!("(unavailable: {err})"))
        );
    } else if !response.latest_journal.is_empty() {
        println!("{}:", p.muted("latest_journal"));
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
    let p = Palette::new();
    if report.dropped_lines > 0 {
        eprintln!(
            "warning: {} malformed journal line(s) dropped",
            p.warn(&report.dropped_lines.to_string())
        );
    }
    if report.entries.is_empty() {
        println!("{}: 0 journal entries", p.primary(&report.service));
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
        println!("{} {}: {}", p.muted(timestamp), p.primary(ident), message);
    }
    Ok(())
}

pub(crate) fn print_incident_response(response: &IncidentResponse, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "incident around {} +/- {}m: {} event(s){}",
        p.muted(&response.around),
        p.cyan(&response.window_minutes.to_string()),
        p.cyan(&response.event_count.to_string()),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    for warning in &response.warnings {
        println!("{}: {}", p.warn("warn"), warning);
    }
    for event in &response.events {
        let host = event.host.as_deref().unwrap_or("-");
        let severity = event.severity.as_deref().unwrap_or("-");
        let app = event.app.as_deref().unwrap_or("-");
        println!(
            "{} {} {} {} {}: {}",
            p.muted(&local_ts(&event.timestamp)),
            p.primary(&event.source),
            p.cyan(host),
            p.severity(severity),
            p.primary(app),
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
    let p = Palette::new();
    println!(
        "{}: {}",
        p.muted("session_id"),
        p.primary(&response.session_id)
    );
    println!(
        "{}: {}",
        p.muted("transcript_path"),
        p.primary(&response.transcript_path.display().to_string())
    );
    println!(
        "{}: {}",
        p.muted("ingested"),
        p.cyan(&response.ingested.to_string())
    );
    println!(
        "{}: {}",
        p.muted("pruned_missing_checkpoint"),
        p.primary(&response.pruned_missing_checkpoint.to_string())
    );
    println!(
        "{}: {}",
        p.muted("missing_checkpoint_count"),
        if response.missing_checkpoint_count > 0 {
            p.warn(&response.missing_checkpoint_count.to_string())
                .to_string()
        } else {
            p.cyan(&response.missing_checkpoint_count.to_string())
                .to_string()
        }
    );
    Ok(())
}

pub(crate) fn print_index_response(response: &IndexResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "files={} ingested={} duplicates={} parse_errors={} skipped={} unsupported={} symlinks={} unsafe_paths={} storage_blocked_chunks={} dropped_metadata_fields={} checkpoint_updates={} file_errors={}",
        p.cyan(&response.discovered_files.to_string()),
        p.cyan(&response.ingested.to_string()),
        p.muted(&response.skipped_dupes.to_string()),
        if response.parse_errors > 0 { p.error(&response.parse_errors.to_string()).to_string() } else { p.muted(&response.parse_errors.to_string()).to_string() },
        p.muted(&response.skipped_files.to_string()),
        p.muted(&response.unsupported_files.to_string()),
        p.muted(&response.skipped_symlinks.to_string()),
        p.muted(&response.skipped_unsafe_paths.to_string()),
        if response.storage_blocked_chunks > 0 { p.error(&response.storage_blocked_chunks.to_string()).to_string() } else { p.muted(&response.storage_blocked_chunks.to_string()).to_string() },
        p.muted(&response.dropped_metadata_fields.to_string()),
        p.cyan(&response.checkpoint_updates.to_string()),
        if !response.file_errors.is_empty() { p.error(&response.file_errors.len().to_string()).to_string() } else { p.muted(&response.file_errors.len().to_string()).to_string() }
    );
    for error in &response.file_errors {
        eprintln!(
            "{}: {}: {}",
            p.error("index error"),
            p.primary(&error.path),
            error.error
        );
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
