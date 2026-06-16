//! Shell completion script generation (`cortex completions <shell>`).

pub(crate) fn zsh_completion_script() -> &'static str {
    include_str!("completions/_cortex.zsh")
}

/// Print the completion script for `shell` to stdout, or error for unsupported
/// shells.
pub(crate) fn print_completions(shell: &str) -> anyhow::Result<()> {
    match shell {
        "zsh" => {
            println!("{}", zsh_completion_script());
            Ok(())
        }
        other => anyhow::bail!("unsupported shell '{other}'; supported: zsh"),
    }
}

#[cfg(test)]
#[path = "completions_tests.rs"]
mod tests;
