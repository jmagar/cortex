use anyhow::{bail, Result};
use serde::Serialize;
use std::net::TcpListener;
use std::path::PathBuf;

use super::output_common::print_json;
use super::{PluginHookArgs, SetupCommand};

pub(crate) fn run_setup(command: SetupCommand) -> Result<()> {
    match command {
        SetupCommand::Check(args) => {
            let report = setup_report(SetupMode::Check)?;
            print_setup_report(&report, args.json)?;
            ensure_setup_success(&report)
        }
        SetupCommand::Repair(args) => {
            let report = setup_report(SetupMode::Repair)?;
            print_setup_report(&report, args.json)?;
            ensure_setup_success(&report)
        }
        SetupCommand::Install(_args) => {
            let dest = install_self()?;
            println!("installed -> {}", dest.display());
            Ok(())
        }
        SetupCommand::PluginHook(args) => run_plugin_hook(args),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SetupMode {
    Check,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SetupStatus {
    Ok,
    Warn,
    Error,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]

pub(crate) struct SetupPhase {
    pub(crate) name: &'static str,
    pub(crate) status: SetupStatus,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, Serialize)]

struct SetupReport {
    mode: SetupMode,
    data_dir: PathBuf,
    env_path: PathBuf,
    phases: Vec<SetupPhase>,
    has_errors: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PluginHookExitPolicy {
    Success,
    AdvisoryFailure,
    BlockingFailure,
}

#[derive(Debug, Clone, Serialize)]

struct PluginHookReport {
    exit_policy: PluginHookExitPolicy,
    ran_repair: bool,
    no_repair: bool,
    blocking_failures: Vec<String>,
    advisory_failures: Vec<String>,
    check: SetupReport,
    repair: Option<SetupReport>,
}

/// Copy the running binary into `~/.local/bin/<name>` so it is callable as a
/// bare command in the user's own terminal, independent of Claude Code. Copy
/// (not symlink) so it survives `/plugin update`. std + anyhow only.
pub(crate) fn install_self() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    let name = exe
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("cannot determine binary name from {}", exe.display()))?;
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let bin_dir = std::path::PathBuf::from(home).join(".local").join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let dest = bin_dir.join(name);
    if dest == exe {
        return Ok(dest);
    }
    let tmp = bin_dir.join(format!(".{}.tmp", name.to_string_lossy()));
    std::fs::copy(&exe, &tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &dest).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d == bin_dir))
        .unwrap_or(false);
    if !on_path {
        eprintln!("note: {} is not on your PATH; add:  export PATH=\"$HOME/.local/bin:$PATH\"", bin_dir.display());
    }
    Ok(dest)
}

fn run_plugin_hook(args: PluginHookArgs) -> Result<()> {
    // Keep the user's terminal copy in ~/.local/bin fresh each session.
    if let Err(e) = install_self() {
        eprintln!("cortex setup plugin-hook: self-install skipped: {e}");
    }
    let check = setup_report(SetupMode::Check)?;
    let repair = if check.has_errors && !args.no_repair {
        Some(setup_report(SetupMode::Repair)?)
    } else {
        None
    };
    let active = repair.as_ref().unwrap_or(&check);
    let blocking_failures = setup_blocking_failures(active);
    let advisory_failures = setup_advisory_failures(active);
    let exit_policy = if !blocking_failures.is_empty() {
        PluginHookExitPolicy::BlockingFailure
    } else if !advisory_failures.is_empty() {
        PluginHookExitPolicy::AdvisoryFailure
    } else {
        PluginHookExitPolicy::Success
    };
    let report = PluginHookReport {
        exit_policy,
        ran_repair: repair.is_some(),
        no_repair: args.no_repair,
        blocking_failures,
        advisory_failures,
        check,
        repair,
    };
    print_plugin_hook_report(&report, args.json)?;
    if matches!(report.exit_policy, PluginHookExitPolicy::BlockingFailure) {
        bail!(
            "cortex setup plugin-hook completed with blocking failed phases: {}",
            report.blocking_failures.join(", ")
        );
    }
    Ok(())
}

fn setup_report(mode: SetupMode) -> Result<SetupReport> {
    let data_dir = setup_data_dir();
    let env_path = data_dir.join(".env");

    if matches!(mode, SetupMode::Repair) {
        std::fs::create_dir_all(&data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&data_dir)?.permissions();
            perms.set_mode(0o700);
            std::fs::set_permissions(&data_dir, perms)?;
        }
    }

    let mut phases = Vec::new();
    phases.push(if data_dir.is_dir() {
        SetupPhase {
            name: "data-dir",
            status: SetupStatus::Ok,
            detail: format!("found {}", data_dir.display()),
        }
    } else {
        SetupPhase {
            name: "data-dir",
            status: SetupStatus::Error,
            detail: format!("missing {}; run cortex setup repair", data_dir.display()),
        }
    });
    phases.push(if env_path.exists() {
        SetupPhase {
            name: "env",
            status: SetupStatus::Ok,
            detail: format!("found {}", env_path.display()),
        }
    } else {
        SetupPhase {
            name: "env",
            status: SetupStatus::Warn,
            detail: format!(
                "missing {}; plugin env may be supplied by process",
                env_path.display()
            ),
        }
    });
    phases.push(
        if std::env::var("CORTEX_TOKEN").is_ok()
            || std::env::var("CORTEX_API_TOKEN").is_ok()
            || std::env::var("NO_AUTH").ok().as_deref() == Some("true")
        {
            SetupPhase {
                name: "auth",
                status: SetupStatus::Ok,
                detail: "token/no_auth configuration present".to_string(),
            }
        } else {
            SetupPhase {
                name: "auth",
                status: SetupStatus::Warn,
                detail: "no CORTEX_TOKEN/CORTEX_API_TOKEN in process env".to_string(),
            }
        },
    );
    phases.push(mcp_port_phase());
    // data_mount_phase intentionally NOT included here (bead cortex-0p8r.11).
    // Post-cutover (CORTEX_USE_HTTP=true is the default), the CLI no longer
    // opens SQLite directly, so the SessionStart cost of docker inspect is no
    // longer paying for itself. Drift detection is preserved via:
    //   - `cortex compose doctor`           (always runs coord phases)
    //   - `cortex db status --check-coord`  (opt-in)
    // See bead cortex-0p8r.13 for the coord-phase wiring.

    let has_errors = phases
        .iter()
        .any(|phase| matches!(phase.status, SetupStatus::Error));
    Ok(SetupReport {
        mode,
        data_dir,
        env_path,
        phases,
        has_errors,
    })
}

/// Minimal `.env` parser: reads KEY=VALUE lines, ignores comments and quotes.
/// Returns the unquoted value if `key` is present.
pub(crate) fn read_env_value(path: &std::path::Path, key: &str) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() == key {
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(v);
            let v = v
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .unwrap_or(v);
            return Some(v.to_string());
        }
    }
    None
}

pub(crate) fn setup_data_dir() -> PathBuf {
    std::env::var_os("CORTEX_DATA_DIR")
        .or_else(|| std::env::var_os("CLAUDE_PLUGIN_DATA"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cortex")))
        .unwrap_or_else(|| PathBuf::from(".cortex"))
}

fn mcp_port_phase() -> SetupPhase {
    let port = setup_port("CORTEX_PORT", 3100);
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => SetupPhase {
            name: "mcp-port",
            status: SetupStatus::Ok,
            detail: format!("port {port} available"),
        },
        Err(error) => SetupPhase {
            name: "mcp-port",
            status: SetupStatus::Warn,
            detail: format!("port {port} is already in use: {error}"),
        },
    }
}

fn setup_port(env_name: &str, default: u16) -> u16 {
    std::env::var(env_name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn setup_blocking_failures(report: &SetupReport) -> Vec<String> {
    report
        .phases
        .iter()
        .filter(|phase| matches!(phase.status, SetupStatus::Error))
        .map(|phase| phase.name.to_string())
        .collect()
}

fn setup_advisory_failures(report: &SetupReport) -> Vec<String> {
    report
        .phases
        .iter()
        .filter(|phase| matches!(phase.status, SetupStatus::Warn))
        .map(|phase| phase.name.to_string())
        .collect()
}

fn ensure_setup_success(report: &SetupReport) -> Result<()> {
    if report.has_errors {
        bail!("cortex setup completed with failed phases");
    }
    Ok(())
}

fn print_setup_report(report: &SetupReport, json: bool) -> Result<()> {
    if json {
        return print_json(report);
    }
    println!("Syslog setup mode: {:?}", report.mode);
    println!("Data dir: {}", report.data_dir.display());
    println!("Env: {}", report.env_path.display());
    for phase in &report.phases {
        println!("{:?}\t{}\t{}", phase.status, phase.name, phase.detail);
    }
    Ok(())
}

fn print_plugin_hook_report(report: &PluginHookReport, json: bool) -> Result<()> {
    if json {
        return print_json(report);
    }
    print_setup_report(&report.check, false)?;
    if let Some(repair) = &report.repair {
        print_setup_report(repair, false)?;
    }
    println!("Plugin hook policy: {:?}", report.exit_policy);
    println!("Plugin hook ran repair: {}", report.ran_repair);
    Ok(())
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
