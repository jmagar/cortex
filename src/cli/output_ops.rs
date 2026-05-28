use anyhow::{bail, Result};
use serde::Serialize;
use syslog_mcp::app::{
    DbBackupResult, DbCheckpointResult, DbIntegrityResult, DbMaintenanceStatus, DbVacuumResult,
};
use syslog_mcp::compose::{CommandOutput, ComposeCommandResult, ComposeStatus};

use super::color::Palette;
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
    let p = Palette::new();
    println!(
        "{}: {}",
        p.muted("db_path"),
        p.primary(&status.db_path.display().to_string())
    );
    println!(
        "{}: {}",
        p.muted("page_count"),
        p.cyan(&status.page_count.to_string())
    );
    println!(
        "{}: {}",
        p.muted("freelist_count"),
        p.cyan(&status.freelist_count.to_string())
    );
    println!(
        "{}: {}",
        p.muted("page_size"),
        p.cyan(&status.page_size.to_string())
    );
    println!(
        "{}: {}",
        p.muted("logical_size_bytes"),
        p.cyan(&status.logical_size_bytes.to_string())
    );
    println!(
        "{}: {}",
        p.muted("physical_size_bytes"),
        p.cyan(&status.physical_size_bytes.to_string())
    );
    println!(
        "{}: {}",
        p.muted("wal_size_bytes"),
        p.cyan(
            &status
                .wal_size_bytes
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string())
        )
    );
    println!(
        "{}: {}",
        p.muted("shm_size_bytes"),
        p.cyan(
            &status
                .shm_size_bytes
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string())
        )
    );
    println!(
        "{}: {}",
        p.muted("auto_vacuum"),
        p.cyan(&status.auto_vacuum.to_string())
    );
    println!(
        "{}: {}",
        p.muted("journal_mode"),
        p.primary(&status.journal_mode)
    );
    let integrity_str = status
        .integrity_ok
        .map(|v| v.to_string())
        .unwrap_or_else(|| "not checked".to_string());
    let integrity_colored = match status.integrity_ok {
        Some(true) => p.success(&integrity_str).to_string(),
        Some(false) => p.error(&integrity_str).to_string(),
        None => p.muted(&integrity_str).to_string(),
    };
    println!("{}: {}", p.muted("integrity_ok"), integrity_colored);
    if let Some(phases) = coordination {
        println!();
        println!("{}:", p.muted("coordination"));
        for phase in phases {
            println!(
                "  {} {} — {}",
                phase_status_colored(&p, &phase.status),
                p.primary(phase.name),
                p.muted(&phase.detail)
            );
        }
    }
    Ok(())
}

pub(crate) fn print_db_integrity_response(response: &DbIntegrityResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    let ok_str = response.ok.to_string();
    let ok_colored = if response.ok {
        p.success(&ok_str).to_string()
    } else {
        p.error(&ok_str).to_string()
    };
    println!("{}: {}", p.muted("ok"), ok_colored);
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
    let p = Palette::new();
    println!("{}: {}", p.muted("mode"), p.primary(&response.mode));
    println!(
        "{}: {}",
        p.muted("busy"),
        p.primary(&response.busy.to_string())
    );
    println!(
        "{}: {}",
        p.muted("log_frames"),
        p.cyan(&response.log_frames.to_string())
    );
    println!(
        "{}: {}",
        p.muted("checkpointed_frames"),
        p.cyan(&response.checkpointed_frames.to_string())
    );
    Ok(())
}

pub(crate) fn print_db_vacuum_response(response: &DbVacuumResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{}: {}",
        p.muted("full"),
        p.primary(&response.full.to_string())
    );
    println!(
        "{}: {}",
        p.muted("incremental_pages"),
        p.cyan(&response.incremental_pages.to_string())
    );
    println!(
        "{}: {}",
        p.muted("before_physical_size_bytes"),
        p.cyan(&response.before_physical_size_bytes.to_string())
    );
    println!(
        "{}: {}",
        p.muted("after_physical_size_bytes"),
        p.cyan(&response.after_physical_size_bytes.to_string())
    );
    Ok(())
}

pub(crate) fn print_db_backup_response(response: &DbBackupResult, json: bool) -> Result<()> {
    if json {
        return print_json(response);
    }
    let p = Palette::new();
    println!(
        "{}: {}",
        p.muted("db_path"),
        p.primary(&response.db_path.display().to_string())
    );
    println!(
        "{}: {}",
        p.muted("backup_path"),
        p.primary(&response.backup_path.display().to_string())
    );
    println!(
        "{}: {}",
        p.muted("size_bytes"),
        p.cyan(&response.size_bytes.to_string())
    );
    Ok(())
}

pub(crate) fn print_compose_status_response(status: &ComposeStatus, json: bool) -> Result<()> {
    if json {
        return print_json(status);
    }
    let p = Palette::new();
    println!(
        "{}: {}",
        p.muted("Container"),
        p.primary(&status.container_name)
    );
    if let Some(value) = &status.status {
        println!("{}: {}", p.muted("Status"), p.primary(value));
    }
    if let Some(value) = &status.health {
        println!("{}: {}", p.muted("Docker health"), p.primary(value));
    }
    if let Some(value) = &status.image {
        println!("{}: {}", p.muted("Image"), p.primary(value));
    }
    if let Some(value) = &status.compose_project {
        println!("{}: {}", p.muted("Compose project"), p.primary(value));
    }
    if let Some(value) = &status.compose_working_dir {
        println!(
            "{}: {}",
            p.muted("Compose working dir"),
            p.primary(&value.display().to_string())
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
    let p = Palette::new();
    print_compose_status_response(status, false)?;
    println!();
    println!("{}:", p.muted("coordination"));
    for phase in coordination {
        println!(
            "  {} {} — {}",
            phase_status_colored(&p, &phase.status),
            p.primary(phase.name),
            p.muted(&phase.detail)
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
                let p = Palette::new();
                println!("Dry run passed: {}", p.primary(&dry_run.command.join(" ")));
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

fn phase_status_colored(p: &Palette, status: &SetupStatus) -> String {
    match status {
        SetupStatus::Ok => p.success("ok").to_string(),
        SetupStatus::Warn => p.warn("warn").to_string(),
        SetupStatus::Error => p.error("error").to_string(),
        SetupStatus::Skipped => p.muted("skipped").to_string(),
    }
}

#[cfg(test)]
#[path = "output_ops_tests.rs"]
mod tests;
