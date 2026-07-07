use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{
    PhaseTimer, SetupPhase, SetupReport, SetupStatus, host_local_report_input, setup_report,
    write_executable_file,
};

const ZSH_COMPLETION_SCRIPT: &str = include_str!("../cli/completions/_cortex.zsh");

pub async fn run_shell_completions_setup(
    action: super::ShellCompletionsAction,
) -> io::Result<SetupReport> {
    let started = Instant::now();
    let home = super::cortex_home_dir()?;
    let env_path = home.join(".env");
    let compose_dir = home.join("compose");
    let data_dir = home.join("data");
    let user_home = super::user_home_dir()?;
    let install_path = shell_completions_install_path(&user_home);
    let mut phases = Vec::new();

    match action {
        super::ShellCompletionsAction::Install => {
            phases.push(install_shell_completions(&install_path)?);
            phases.push(shell_completions_fpath_hint_phase(&user_home));
        }
        super::ShellCompletionsAction::Remove => {
            phases.push(remove_shell_completions(&install_path)?);
        }
        super::ShellCompletionsAction::Check => {
            phases.push(check_shell_completions_content_phase(&install_path));
            phases.push(shell_completions_fpath_hint_phase(&user_home));
        }
    }

    let elapsed_ms = started.elapsed().as_millis();
    Ok(setup_report(
        host_local_report_input(
            action.as_str(),
            elapsed_ms,
            home,
            env_path,
            compose_dir,
            data_dir,
        ),
        phases,
    ))
}

fn shell_completions_install_path(user_home: &Path) -> PathBuf {
    user_home.join(".local/share/cortex/completions/_cortex")
}

fn install_shell_completions(install_path: &Path) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("shell-completions-files");
    if let Some(parent) = install_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_executable_file(install_path, ZSH_COMPLETION_SCRIPT)?;
    Ok(timer.finish(SetupStatus::Ok, format!("wrote {}", install_path.display())))
}

fn remove_shell_completions(install_path: &Path) -> io::Result<SetupPhase> {
    let timer = PhaseTimer::start("shell-completions-wrapper");
    match std::fs::remove_file(install_path) {
        Ok(()) => Ok(timer.finish(
            SetupStatus::Ok,
            format!("removed {}", install_path.display()),
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(timer.finish(
            SetupStatus::Ok,
            format!("{} already absent", install_path.display()),
        )),
        Err(error) => Err(error),
    }
}

fn check_shell_completions_content_phase(install_path: &Path) -> SetupPhase {
    let timer = PhaseTimer::start("shell-completions-content");
    match std::fs::read_to_string(install_path) {
        Ok(current) if current == ZSH_COMPLETION_SCRIPT => {
            timer.finish(SetupStatus::Ok, "completion script matches generated content")
        }
        Ok(_) => timer.finish(
            SetupStatus::Error,
            format!(
                "{} does not match generated completion script; run cortex setup shell completions install",
                install_path.display()
            ),
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => timer.finish(
            SetupStatus::Warn,
            format!(
                "missing {}; run cortex setup shell completions install",
                install_path.display()
            ),
        ),
        Err(error) => timer.finish(SetupStatus::Error, error.to_string()),
    }
}

/// Read-only check: does `~/.zshrc` appear to add the completions directory
/// to `fpath`? `~/.zshrc` is chezmoi-managed on Jacob's hosts (see the
/// homelab CLAUDE.md's chezmoi rules) — this function never writes to it,
/// only warns with the exact line the operator should add themselves.
fn shell_completions_fpath_hint_phase(user_home: &Path) -> SetupPhase {
    let timer = PhaseTimer::start("shell-completions-fpath");
    let zshrc = user_home.join(".zshrc");
    let expected_dir = user_home.join(".local/share/cortex/completions");
    let expected_dir = expected_dir.display().to_string();
    match std::fs::read_to_string(&zshrc) {
        Ok(content) if content.contains(&expected_dir) => timer.finish(
            SetupStatus::Ok,
            format!("{expected_dir} already sourced from ~/.zshrc"),
        ),
        Ok(_) => timer.finish(
            SetupStatus::Warn,
            format!(
                "add `fpath+=({expected_dir}); autoload -Uz compinit && compinit` to ~/.zshrc (chezmoi-managed; not edited automatically)"
            ),
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => timer.finish(
            SetupStatus::Warn,
            format!(
                "~/.zshrc not found; add `fpath+=({expected_dir}); autoload -Uz compinit && compinit` to your zsh init"
            ),
        ),
        Err(error) => timer.finish(SetupStatus::Warn, error.to_string()),
    }
}

#[cfg(test)]
#[path = "shell_completions_tests.rs"]
mod tests;
