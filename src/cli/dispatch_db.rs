use super::dispatch::http_or_cancel;

use anyhow::{Result, bail};
use cortex::app::{DbBackupRequest, DbCheckpointRequest, DbIntegrityRequest, DbVacuumRequest};

use super::DbIntegrityStatusArgs;
use super::output_ops::{print_db_integrity_job_started, print_db_integrity_job_status};
use std::path::PathBuf;
use std::time::Duration;

use super::coordination::run_coordination_phases;
use super::output_ops::{
    print_db_backup_response, print_db_checkpoint_response, print_db_integrity_response,
    print_db_status_response, print_db_vacuum_response,
};
use super::{CliMode, DbBackupArgs, DbCheckpointArgs, DbIntegrityArgs, DbStatusArgs, DbVacuumArgs};

/// HTTP-side timeout for `db integrity`. On a 31 GB+ DB, `PRAGMA quick_check`
/// (let alone full `integrity_check`) reads every page and can exceed the
/// global 600s `REQUEST_TIMEOUT`. This shorter gate fires first and emits an
/// actionable message directing the operator to run the check inside the
/// container, where the local path has no timeout.
///
/// (bead cortex-qekb) — 120s is roughly a 10 GB/min I/O estimate, i.e.
/// it is expected to complete on a DB up to ~20 GB; anything larger gets the
/// actionable message.
pub(crate) const INTEGRITY_HTTP_TIMEOUT: Duration = Duration::from_secs(120);

/// HTTP-side timeout for `db backup`. A full online backup of a 31 GB+ DB
/// can take several minutes due to the 50 ms inter-step sleep cadence used
/// to let WAL writers proceed between steps. 600s (10 min) gives ample
/// headroom for large databases while still giving an actionable error for
/// hung or unreachable servers.
pub(crate) const BACKUP_HTTP_TIMEOUT: Duration = Duration::from_secs(600);

// ─── DB Arg → Request conversions (bead 0p8r.9) ─────────────────────────────
//
// DbIntegrityArgs / DbCheckpointArgs were identity maps to their *Request
// counterparts (bead 0p8r.29). Inlined at the call sites. DbVacuumArgs keeps
// `into_request` because `bool → Option<bool>` is non-trivial.

impl DbVacuumArgs {
    /// CLI `force: bool` maps to server `Option<bool>` as
    /// `true → Some(true)`, `false → None` (NOT `Some(false)`). The size
    /// pre-flight on `--full` is bypassed only when the body carries
    /// `Some(true)`. `None` and `Some(false)` are equivalent on the wire and
    /// both leave the pre-flight in force. See [`DbVacuumRequest`] docs and
    /// bead 0p8r.4 eng-review C3.
    pub(crate) fn into_request(self) -> DbVacuumRequest {
        DbVacuumRequest {
            full: self.full,
            incremental_pages: self.pages,
            force: if self.force { Some(true) } else { None },
        }
    }
}

// ─── DB Per-command dispatch (bead 0p8r.9) ──────────────────────────────────

pub(crate) async fn run_db_status(mode: &CliMode, args: DbStatusArgs) -> Result<()> {
    let response = match mode {
        CliMode::Local(service) => service.db_status().await?,
        CliMode::Http(client) => http_or_cancel(client.db_status()).await?,
    };
    // Coordination phases shell out to docker/systemctl on the host. They
    // make sense in either mode — even with --http, the operator may want
    // to verify that the host's ai-watch unit agrees with the container's
    // /data bind. Keep the opt-in flag mode-agnostic.
    let coordination = if args.check_coord {
        Some(run_coordination_phases())
    } else {
        None
    };
    print_db_status_response(&response, coordination.as_deref(), args.json)
}

pub(crate) async fn run_db_integrity(mode: &CliMode, args: DbIntegrityArgs) -> Result<()> {
    run_db_integrity_with_timeout(mode, args, INTEGRITY_HTTP_TIMEOUT).await
}

/// Testable inner form of [`run_db_integrity`] — accepts an injected HTTP
/// timeout so the timeout-fires path can be exercised in unit tests without
/// waiting 120 seconds.
pub(crate) async fn run_db_integrity_with_timeout(
    mode: &CliMode,
    args: DbIntegrityArgs,
    http_timeout: Duration,
) -> Result<()> {
    let DbIntegrityArgs {
        quick,
        json,
        background,
    } = args;
    let req = DbIntegrityRequest { quick };

    // Background mode is server-hosted (the CLI process can't outlive the
    // request to hold the detached job), so it requires HTTP transport.
    if background {
        let CliMode::Http(client) = mode else {
            bail!(
                "db integrity --background requires --http (the job runs server-side).\n\
                 Without --http the check runs synchronously in this process."
            );
        };
        let started = http_or_cancel(client.db_integrity_background(&req)).await?;
        print_db_integrity_job_started(&started, json)?;
        return Ok(());
    }

    let response = match mode {
        CliMode::Local(service) => service.db_integrity(quick).await?,
        CliMode::Http(client) => {
            match tokio::time::timeout(http_timeout, http_or_cancel(client.db_integrity(&req)))
                .await
            {
                Ok(result) => result?,
                Err(_elapsed) => {
                    bail!(
                        "Integrity check timed out after {}s (DB may be very large).\n\
                         Run the check inside the container where there is no timeout:\n\
                         \n\
                         \tdocker exec cortex cortex db integrity --quick\n\
                         \n\
                         This operation may take 10-30 minutes on a 31 GB+ database.",
                        http_timeout.as_secs()
                    );
                }
            }
        }
    };
    print_db_integrity_response(&response, json)?;
    if !response.ok {
        bail!("database integrity check failed");
    }
    Ok(())
}

/// Poll a background integrity job (`db integrity status <id>`). HTTP-only — the
/// job lives in the server's DB, which a local CLI process can also read, but
/// the start path is HTTP-only so the status path matches it.
pub(crate) async fn run_db_integrity_status(
    mode: &CliMode,
    args: DbIntegrityStatusArgs,
) -> Result<()> {
    let DbIntegrityStatusArgs { job_id, json } = args;
    let status = match mode {
        CliMode::Local(service) => service.db_integrity_job_status(job_id).await?,
        CliMode::Http(client) => http_or_cancel(client.db_integrity_job(job_id)).await?,
    };
    print_db_integrity_job_status(&status, json)?;
    // A terminal failed/not-ok job exits non-zero so scripts can branch on it.
    if status.status == "failed" {
        bail!("integrity job {job_id} failed");
    }
    if status.status == "done" && status.integrity.as_ref().is_some_and(|r| !r.ok) {
        bail!("database integrity check failed");
    }
    Ok(())
}

pub(crate) async fn run_db_checkpoint(mode: &CliMode, args: DbCheckpointArgs) -> Result<()> {
    let DbCheckpointArgs {
        mode: chk_mode,
        json,
    } = args;
    let req = DbCheckpointRequest {
        mode: chk_mode.clone(),
    };
    let response = match mode {
        CliMode::Local(service) => service.db_checkpoint_checked(req.clone()).await?,
        CliMode::Http(client) => http_or_cancel(client.db_checkpoint(&req)).await?,
    };
    print_db_checkpoint_response(&response, json)?;
    if response.busy != 0 {
        bail!("database WAL checkpoint was busy");
    }
    Ok(())
}

pub(crate) async fn run_db_vacuum(mode: &CliMode, args: DbVacuumArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => {
            service
                .db_vacuum_checked(req.clone(), crate::api::FULL_VACUUM_SIZE_GUARD_BYTES)
                .await?
        }
        CliMode::Http(client) => http_or_cancel(client.db_vacuum(&req)).await?,
    };
    print_db_vacuum_response(&response, json)
}

pub(crate) async fn run_db_backup(mode: &CliMode, args: DbBackupArgs) -> Result<()> {
    match mode {
        CliMode::Http(client) => {
            // Route through the server — the server holds the pool connection
            // so the rusqlite backup API cooperates with WAL writers; no lock
            // conflict even when the container is actively ingesting logs.
            //
            // Note: `output_path` is a **server-side** path (e.g.
            // `/data/backup.db` inside the container, visible on the host via
            // the Docker bind-mount). Pass `None` to let the server choose.
            //
            // Warn the operator (bead xknb.5): in HTTP mode `--output` is
            // resolved by the *server* process, not the local shell. A path
            // like `/tmp/backup.db` lands inside the container, not on the host.
            if let Some(path) = args.output.as_deref() {
                // Sanitize before writing to the terminal (same hardening as the
                // xknb.4 audit log): `--output` is operator-supplied and may carry
                // CR/LF/ESC; strip them so a crafted path can't inject newlines or
                // ANSI escapes into stderr.
                let path = path.replace(['\n', '\r', '\x1b'], "?");
                eprintln!(
                    "warning: --output '{path}' is a SERVER-SIDE path resolved inside \
                     the server process (e.g. the container), not your local shell. \
                     For a host-visible file use a path under the server's /data mount."
                );
            }
            let req = DbBackupRequest {
                output_path: args.output.clone(),
            };
            let response = match tokio::time::timeout(
                BACKUP_HTTP_TIMEOUT,
                http_or_cancel(client.db_backup(&req)),
            )
            .await
            {
                Ok(result) => result?,
                Err(_elapsed) => {
                    bail!(
                        "Backup timed out after {}s (DB may be very large).\n\
                             Run the backup inside the container where there is no timeout:\n\
                             \n\
                             \tdocker exec cortex cortex db backup --output /data/backup-$(date +%Y%m%d).db\n\
                             \n\
                             This operation may take several minutes on a 31 GB+ database.",
                        BACKUP_HTTP_TIMEOUT.as_secs()
                    );
                }
            };
            print_db_backup_response(&response, args.json)
        }
        CliMode::Local(service) => {
            let response = service.db_backup(args.output.map(PathBuf::from)).await;
            match response {
                Ok(r) => print_db_backup_response(&r, args.json),
                Err(e) => {
                    let msg = e.to_string();
                    // SQLITE_BUSY / "database is locked" means the container is
                    // running and holds a WAL write lock. Guide the operator.
                    if msg.contains("database is locked")
                        || msg.contains("SQLITE_BUSY")
                        || msg.contains("unable to open database file")
                    {
                        bail!(
                            "{msg}\n\n\
                             The container is likely running and holds the SQLite write lock.\n\
                             To backup through the running server (recommended):\n\
                             \n\
                             \tcortex --http db backup --output /data/backup-$(date +%Y%m%d).db\n\
                             \n\
                             Or backup inside the container directly:\n\
                             \n\
                             \tdocker exec cortex cortex db backup --output /data/backup-$(date +%Y%m%d).db"
                        )
                    } else {
                        Err(e.into())
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "dispatch_db_tests.rs"]
mod tests;
