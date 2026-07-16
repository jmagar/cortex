//! Declarative positional binding driven by `ACTION_SPECS`.
//!
//! These helpers keep the per-action `parse_*` functions free of bespoke
//! positional logic: each parser collects its leftover positional
//! tokens, then asks this module what they mean for the given action. The
//! action name may be passed in either CLI (`host-state`) or MCP (`host_state`)
//! form — the registry facade normalises hyphens to underscores.

use anyhow::{Result, bail};

/// Bind the collected positional tokens for `action` to its positional flag's
/// value.
///
/// - `Ok(None)` — the action has a positional but none was supplied (leave the
///   field at its parsed/default value), or the action takes no positional and
///   none was supplied.
/// - `Ok(Some(v))` — the single positional `v` to bind to the action's
///   positional flag.
/// - `Err` — positionals were supplied but the action accepts none, or more
///   than one positional was supplied.
pub(crate) fn positional_value(action: &str, positionals: &[String]) -> Result<Option<String>> {
    let accepts = crate::cli::registry_positional(action).is_some();
    match (accepts, positionals.len()) {
        (false, 0) => Ok(None),
        (false, _) => bail!(
            "unexpected argument '{}'; this command takes no positional argument",
            positionals[0]
        ),
        (true, 0) => Ok(None),
        (true, 1) => Ok(Some(positionals[0].clone())),
        (true, _) => bail!(
            "expected at most one positional argument, got {}",
            positionals.len()
        ),
    }
}

#[cfg(test)]
#[path = "argdefaults_tests.rs"]
mod tests;
