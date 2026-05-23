use anyhow::{bail, Result};
use serde::Serialize;
use syslog_mcp::app::{
    DbBackupResult, DbCheckpointResult, DbIntegrityResult, DbMaintenanceStatus, DbVacuumResult,
};
use syslog_mcp::compose::{CommandOutput, ComposeCommandResult, ComposeStatus};

use super::output_common::print_json;
use super::setup::{SetupPhase, SetupStatus};
#[derive(Debug, Clone, Serialize)]
struct DbStatusReport<'a> {
    #[serde(flatten)]
    status: &'a DbMaintenanceStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    coordination: Option<&'a [SetupPhase]>,
}

pub(crate) fn print_db_status_response(
    status: &DbMaintenanceStatus,
    coordination: Option<&[SetupPhase]>,
    json: bool,
) -> Result<()> {
    if json {
        let report = DbStatusReport {
            status,
            coordination,
        };
        return print_json(&report);
    }
    println!("db_path: {}", status.db_path.display());
    println!("page_count: {}", status.page_count);
    println!("freelist_count: {}", status.freelist_count);
    println!("page_size: {}", status.page_size);
    println!("logical_size_bytes: {}", status.logical_size_bytes);
    println!("physical_size_bytes: {}", status.physical_size_bytes);
    println!(
        "wal_size_bytes: {}",
        status
            .wal_size_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "shm_size_bytes: {}",
        status
            .shm_size_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!("auto_vacuum: {}", status.auto_vacuum);
    println!("journal_mode: {}", status.journal_mode);
    println!(
        "integrity_ok: {}",
        status
            .integrity_ok
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not checked".to_string())
    );
    if let Some(phases) = coordination {
        println!();
        println!("coordination:");
        for phase in phases {
            println!("  {:?} {} — {}", phase.status, phase.name, phase.detail);
        }
    }
    Ok(())
}

pub(crate) fn print_db_integrity_response(response: &DbIntegrityResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("ok: {}", response.ok);
    for message in &response.messages {
        println!("{message}");
    }
    Ok(())
}

pub(crate) fn print_db_checkpoint_response(
    response: &DbCheckpointResult,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("mode: {}", response.mode);
    println!("busy: {}", response.busy);
    println!("log_frames: {}", response.log_frames);
    println!("checkpointed_frames: {}", response.checkpointed_frames);
    Ok(())
}

pub(crate) fn print_db_vacuum_response(response: &DbVacuumResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("full: {}", response.full);
    println!("incremental_pages: {}", response.incremental_pages);
    println!(
        "before_physical_size_bytes: {}",
        response.before_physical_size_bytes
    );
    println!(
        "after_physical_size_bytes: {}",
        response.after_physical_size_bytes
    );
    Ok(())
}

pub(crate) fn print_db_backup_response(response: &DbBackupResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("db_path: {}", response.db_path.display());
    println!("backup_path: {}", response.backup_path.display());
    println!("size_bytes: {}", response.size_bytes);
    Ok(())
}

pub(crate) fn print_compose_status_response(status: &ComposeStatus, json: bool) -> Result<()> {
    if json {
        return print_json(status);
    }
    println!("Container: {}", status.container_name);
    if let Some(value) = &status.status {
        println!("Status: {value}");
    }
    if let Some(value) = &status.health {
        println!("Docker health: {value}");
    }
    if let Some(value) = &status.image {
        println!("Image: {value}");
    }
    if let Some(value) = &status.compose_project {
        println!("Compose project: {value}");
    }
    if let Some(value) = &status.compose_working_dir {
        println!("Compose working dir: {}", value.display());
    }
    for diag in &status.diagnostics {
        println!("{:?}: {} - {}", diag.severity, diag.code, diag.message);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]

struct ComposeDoctorReport<'a> {
    #[serde(flatten)]
    status: &'a ComposeStatus,
    coordination: &'a [SetupPhase],
}

pub(crate) fn print_compose_doctor_response(
    status: &ComposeStatus,
    coordination: &[SetupPhase],
    json: bool,
) -> Result<()> {
    if json {
        let report = ComposeDoctorReport {
            status,
            coordination,
        };
        return print_json(&report);
    }
    print_compose_status_response(status, false)?;
    println!();
    println!("coordination:");
    for phase in coordination {
        println!("  {:?} {} — {}", phase.status, phase.name, phase.detail);
    }
    Ok(())
}

pub(crate) fn ensure_doctor_coordination_ok(phases: &[SetupPhase]) -> Result<()> {
    let failures: Vec<String> = phases
        .iter()
        .filter(|p| matches!(p.status, SetupStatus::Error))
        .map(|p| format!("{} — {}", p.name, p.detail))
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    bail!(
        "compose doctor coordination check failed: {}",
        failures.join("; ")
    );
}

pub(crate) fn print_compose_command_response(
    result: &ComposeCommandResult,
    json: bool,
) -> Result<()> {
    match result {
        ComposeCommandResult::Executed(output) => {
            if json {
                print_json(output)?;
            } else {
                print!("{}", output.stdout);
                eprint!("{}", output.stderr);
            }
            ensure_command_success(output)
        }
        ComposeCommandResult::DryRun(dry_run) => {
            if json {
                print_json(dry_run)?;
            } else {
                println!("Dry run passed: {}", dry_run.command.join(" "));
            }
            Ok(())
        }
    }
}

pub(crate) fn ensure_command_success(output: &CommandOutput) -> Result<()> {
    if output.exit_status == Some(0) && !output.timed_out {
        return Ok(());
    }
    bail!(
        "compose command failed: status={:?} timed_out={} stderr={}",
        output.exit_status,
        output.timed_out,
        output.stderr
    )
}

// ---------------------------------------------------------------------------
// `syslog config` — edit `.env` and `config.toml` from the CLI.

#[cfg(test)]
#[path = "output_ops_tests.rs"]
mod tests;
