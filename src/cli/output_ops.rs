use anyhow::{bail, Result};
use cortex::app::{
    DbBackupResult, DbCheckpointResult, DbIntegrityJobStarted, DbIntegrityResult,
    DbMaintenanceStatus, DbVacuumResult, MaintenanceJobStatus,
};
use cortex::compose::{CommandOutput, ComposeCommandResult, ComposeStatus};
use serde::Serialize;

use super::color::{cyan, error, muted, primary, success, warn};
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
    println!(
        "{}: {}",
        muted("db_path"),
        primary(&status.db_path.display().to_string())
    );
    println!(
        "{}: {}",
        muted("page_count"),
        cyan(&status.page_count.to_string())
    );
    println!(
        "{}: {}",
        muted("freelist_count"),
        cyan(&status.freelist_count.to_string())
    );
    println!(
        "{}: {}",
        muted("page_size"),
        cyan(&status.page_size.to_string())
    );
    println!(
        "{}: {}",
        muted("logical_size_bytes"),
        cyan(&status.logical_size_bytes.to_string())
    );
    println!(
        "{}: {}",
        muted("physical_size_bytes"),
        cyan(&status.physical_size_bytes.to_string())
    );
    println!(
        "{}: {}",
        muted("wal_size_bytes"),
        cyan(
            &status
                .wal_size_bytes
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string())
        )
    );
    println!(
        "{}: {}",
        muted("shm_size_bytes"),
        cyan(
            &status
                .shm_size_bytes
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string())
        )
    );
    println!(
        "{}: {}",
        muted("auto_vacuum"),
        cyan(&status.auto_vacuum.to_string())
    );
    println!(
        "{}: {}",
        muted("journal_mode"),
        primary(&status.journal_mode)
    );
    let integrity_str = status
        .integrity_ok
        .map(|v| v.to_string())
        .unwrap_or_else(|| "not checked".to_string());
    let integrity_colored = match status.integrity_ok {
        Some(true) => success(&integrity_str),
        Some(false) => error(&integrity_str),
        None => muted(&integrity_str),
    };
    println!("{}: {}", muted("integrity_ok"), integrity_colored);
    if let Some(phases) = coordination {
        println!();
        println!("{}:", muted("coordination"));
        for phase in phases {
            println!(
                "  {} {} — {}",
                phase_status_colored(&phase.status),
                primary(phase.name),
                muted(&phase.detail)
            );
        }
    }
    Ok(())
}

pub(crate) fn print_db_integrity_response(response: &DbIntegrityResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let ok_str = response.ok.to_string();
    let ok_colored = if response.ok {
        success(&ok_str)
    } else {
        error(&ok_str)
    };
    println!("{}: {}", muted("ok"), ok_colored);
    for message in &response.messages {
        println!("{message}");
    }
    Ok(())
}

pub(crate) fn print_db_integrity_job_started(
    response: &DbIntegrityJobStarted,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{}: {} ({})",
        muted("integrity job"),
        cyan(&response.job_id.to_string()),
        response.status
    );
    println!(
        "{}",
        muted(&format!(
            "poll with: cortex db integrity status {}",
            response.job_id
        ))
    );
    Ok(())
}

pub(crate) fn print_db_integrity_job_status(
    response: &MaintenanceJobStatus,
    json: bool,
) -> Result<()> {
    if json {
        return print_json(response);
    }
    let status_colored = match response.status.as_str() {
        "done" => success(&response.status),
        "failed" => error(&response.status),
        _ => warn(&response.status),
    };
    println!("{}: {}", muted("job"), cyan(&response.job_id.to_string()));
    println!("{}: {}", muted("kind"), response.kind);
    println!("{}: {}", muted("status"), status_colored);
    println!("{}: {}", muted("started"), response.started_at);
    if let Some(finished) = &response.finished_at {
        println!("{}: {}", muted("finished"), finished);
    }
    if let Some(integrity) = &response.integrity {
        let ok_str = integrity.ok.to_string();
        let ok_colored = if integrity.ok {
            success(&ok_str)
        } else {
            error(&ok_str)
        };
        println!("{}: {}", muted("integrity_ok"), ok_colored);
        for message in &integrity.messages {
            println!("{message}");
        }
    }
    if let Some(err) = &response.error {
        println!("{}: {}", error("error"), err);
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
    println!("{}: {}", muted("mode"), primary(&response.mode));
    println!("{}: {}", muted("busy"), primary(&response.busy.to_string()));
    println!(
        "{}: {}",
        muted("log_frames"),
        cyan(&response.log_frames.to_string())
    );
    println!(
        "{}: {}",
        muted("checkpointed_frames"),
        cyan(&response.checkpointed_frames.to_string())
    );
    Ok(())
}

pub(crate) fn print_db_vacuum_response(response: &DbVacuumResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!("{}: {}", muted("full"), primary(&response.full.to_string()));
    println!(
        "{}: {}",
        muted("incremental_pages"),
        cyan(&response.incremental_pages.to_string())
    );
    println!(
        "{}: {}",
        muted("before_physical_size_bytes"),
        cyan(&response.before_physical_size_bytes.to_string())
    );
    println!(
        "{}: {}",
        muted("after_physical_size_bytes"),
        cyan(&response.after_physical_size_bytes.to_string())
    );
    Ok(())
}

pub(crate) fn print_db_backup_response(response: &DbBackupResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    println!(
        "{}: {}",
        muted("db_path"),
        primary(&response.db_path.display().to_string())
    );
    println!(
        "{}: {}",
        muted("backup_path"),
        primary(&response.backup_path.display().to_string())
    );
    println!(
        "{}: {}",
        muted("size_bytes"),
        cyan(&response.size_bytes.to_string())
    );
    Ok(())
}

pub(crate) fn print_compose_status_response(status: &ComposeStatus, json: bool) -> Result<()> {
    if json {
        return print_json(status);
    }
    println!(
        "{}: {}",
        muted("Container"),
        primary(&status.container_name)
    );
    if let Some(value) = &status.status {
        println!("{}: {}", muted("Status"), primary(value));
    }
    if let Some(value) = &status.health {
        println!("{}: {}", muted("Docker health"), primary(value));
    }
    if let Some(value) = &status.image {
        println!("{}: {}", muted("Image"), primary(value));
    }
    if let Some(value) = &status.compose_project {
        println!("{}: {}", muted("Compose project"), primary(value));
    }
    if let Some(value) = &status.compose_working_dir {
        println!(
            "{}: {}",
            muted("Compose working dir"),
            primary(&value.display().to_string())
        );
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
    println!("{}:", muted("coordination"));
    for phase in coordination {
        println!(
            "  {} {} — {}",
            phase_status_colored(&phase.status),
            primary(phase.name),
            muted(&phase.detail)
        );
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
                println!("Dry run passed: {}", primary(&dry_run.command.join(" ")));
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

fn phase_status_colored(status: &SetupStatus) -> String {
    match status {
        SetupStatus::Ok => success("ok"),
        SetupStatus::Warn => warn("warn"),
        SetupStatus::Error => error("error"),
        SetupStatus::Skipped => muted("skipped"),
    }
}

#[cfg(test)]
#[path = "output_ops_tests.rs"]
mod tests;
