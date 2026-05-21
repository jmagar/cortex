use anyhow::{anyhow, bail, Result};
use syslog_mcp::app::SyslogService;

use super::args::{AiCommand, CliCommand, DbCommand};
use super::dispatch;

/// Env var that opts a process into HTTP transport without passing `--http`.
/// Accepts `1` or `true` (case-insensitive). Any other value is treated as
/// unset to avoid surprising "I typoed `falze`" silent flips.
pub(crate) const ENV_USE_HTTP: &str = "SYSLOG_USE_HTTP";

/// CLI transport mode resolved from global flags + env. Built once per
/// invocation; passed by value into [`run`].
///
/// `Local` keeps the full sqlx + rusqlite + FTS5 stack linked into the host
/// binary — acknowledged limitation, tracked for the v0.30 successor (bead
/// .12 doc note + epic acceptance criteria).
pub(crate) enum CliMode {
    Local(SyslogService),
    Http(super::http_client::HttpClient),
}

impl std::fmt::Debug for CliMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(_) => f.write_str("CliMode::Local(SyslogService)"),
            Self::Http(_) => f.write_str("CliMode::Http(HttpClient)"),
        }
    }
}

/// Top-level dispatch entry point. Built once per CLI invocation by `run_cli`
/// in `main.rs`. The [`CliMode`] decides whether we hit a local SQLite-backed
/// [`SyslogService`] or a remote container via [`HttpClient`].
///
/// HTTP dispatch is implemented incrementally by bead .7+ — for now, the
/// `Http` arm returns a clear placeholder error per command. The mode wiring
/// is in place so .7 can light up commands one by one without touching this
/// signature.
pub(crate) async fn run(mode: CliMode, command: CliCommand) -> Result<()> {
    // Query commands (search/tail/errors/hosts/correlate/stats/sessions) are
    // mode-agnostic: dispatch::run_X branches on `&CliMode` internally and
    // wraps the HTTP path in `http_or_cancel` for SIGINT handling. Everything
    // else (ai/db/compose/setup) still flows through the Local-only path.
    match command {
        CliCommand::Search(args) => dispatch::run_search(&mode, args).await,
        CliCommand::Tail(args) => dispatch::run_tail(&mode, args).await,
        CliCommand::Errors(args) => dispatch::run_errors(&mode, args).await,
        CliCommand::Hosts(args) => dispatch::run_hosts(&mode, args).await,
        CliCommand::Incident(args) => dispatch::run_incident(&mode, args).await,
        CliCommand::Correlate(args) => dispatch::run_correlate(&mode, args).await,
        CliCommand::Stats(args) => dispatch::run_stats(&mode, args).await,
        CliCommand::Sessions(args) => dispatch::run_sessions(&mode, args).await,
        // AI commands (bead 0p8r.8). 10 are HTTP-capable; 6 are LOCAL-only
        // and bail in HTTP mode with a per-command inline message.
        CliCommand::Ai(ai) => match ai {
            AiCommand::Search(args) => dispatch::run_ai_search(&mode, args).await,
            AiCommand::Abuse(args) => dispatch::run_ai_abuse(&mode, args).await,
            AiCommand::Correlate(args) => dispatch::run_ai_correlate(&mode, args).await,
            AiCommand::Blocks(args) => dispatch::run_ai_blocks(&mode, args).await,
            AiCommand::Context(args) => dispatch::run_ai_context(&mode, args).await,
            AiCommand::Tools(args) => dispatch::run_ai_tools(&mode, args).await,
            AiCommand::Projects(args) => dispatch::run_ai_projects(&mode, args).await,
            AiCommand::Checkpoints(args) => dispatch::run_ai_checkpoints(&mode, args).await,
            AiCommand::Errors(args) => dispatch::run_ai_errors(&mode, args).await,
            AiCommand::PruneCheckpoints(args) => {
                dispatch::run_ai_prune_checkpoints(&mode, args).await
            }
            AiCommand::Index(args) => dispatch::run_ai_index(&mode, args).await,
            AiCommand::Add(args) => dispatch::run_ai_add(&mode, args).await,
            AiCommand::Doctor(args) => dispatch::run_ai_doctor(&mode, args).await,
            AiCommand::SmokeWatch(args) => dispatch::run_ai_smoke_watch(&mode, args).await,
            AiCommand::WatchStatus(args) => dispatch::run_ai_watch_status(&mode, args).await,
            AiCommand::Watch(args) => dispatch::run_ai_watch(&mode, args).await,
            AiCommand::Incidents(args) => dispatch::run_ai_incidents(&mode, args).await,
            AiCommand::Investigate(args) => dispatch::run_ai_investigate(&mode, args).await,
            AiCommand::Assess(args) => dispatch::run_ai_assess(&mode, args).await,
        },
        // DB commands (bead 0p8r.9). 4 are HTTP-capable; backup stays LOCAL
        // and bails in HTTP mode with an inline message.
        CliCommand::Db(db) => match db {
            DbCommand::Status(args) => dispatch::run_db_status(&mode, args).await,
            DbCommand::Integrity(args) => dispatch::run_db_integrity(&mode, args).await,
            DbCommand::Checkpoint(args) => dispatch::run_db_checkpoint(&mode, args).await,
            DbCommand::Vacuum(args) => dispatch::run_db_vacuum(&mode, args).await,
            DbCommand::Backup(args) => dispatch::run_db_backup(&mode, args).await,
        },
        // Compose/Setup/Config are local-only and main::run_cli reroutes them BEFORE
        // calling run(). If we reach here, the front door was bypassed —
        // bail with a clear internal-error message rather than a placeholder.
        CliCommand::Compose(_) | CliCommand::Service(_) | CliCommand::Setup(_) => {
            bail!(
                "internal: compose/service/setup must be dispatched by main::run_cli before reaching cli::run()"
            )
        }
        CliCommand::Config(_) => {
            bail!("internal: config commands must be dispatched by main::run_cli before reaching cli::run()")
        }
    }
}

/// Global CLI flags that apply to every subcommand. Stripped from the raw
/// arg list by [`GlobalFlags::extract`] **before** subcommand parsing so the
/// per-command parsers (which we did NOT touch in this bead) keep matching
/// only the flags they already know about.
///
/// `--http` is a bare bool. `--server` and `--token` accept either a separate
/// arg (`--server URL`) or `=`-glued form (`--server=URL`). Passing `--server`
/// or `--token` implies HTTP mode even without an explicit `--http`.
///
/// `SYSLOG_API_TOKEN` alone does **not** flip the default to HTTP — that
/// would silently change behaviour for users who already exported the token
/// from earlier deploys (locked decision, eng-review #C6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GlobalFlags {
    pub force_http: bool,
    pub server: Option<String>,
    pub token: Option<String>,
}

impl GlobalFlags {
    /// Strip global flags out of `args` in-place and return them.
    ///
    /// Unknown args are left in place untouched so the existing per-subcommand
    /// parsers see exactly what they used to. We deliberately allow both
    /// `syslog --http search foo` and `syslog search --http foo` — the
    /// stripper walks the whole vec, not just a prefix.
    ///
    /// `--server` / `--token` without a following value error out; an empty
    /// value (e.g. `--token=`) is also an error so a stray trailing `=` does
    /// not silently produce HTTP mode with a blank token.
    pub(crate) fn extract(args: &mut Vec<String>) -> Result<Self> {
        let mut out = GlobalFlags::default();
        let mut i = 0;
        while i < args.len() {
            // Two flag families: bare "--http", and value-bearing
            // "--server"/"--token" which accept "--flag VALUE" or "--flag=VALUE".
            let arg = args[i].as_str();
            if arg == "--http" {
                out.force_http = true;
                args.remove(i);
                continue;
            }
            if let Some(value) = strip_eq_prefix(arg, "--server") {
                if value.is_empty() {
                    bail!("--server requires a value");
                }
                out.server = Some(value.to_string());
                args.remove(i);
                continue;
            }
            if arg == "--server" {
                if i + 1 >= args.len() {
                    bail!("--server requires a value");
                }
                let value = args.remove(i + 1);
                if value.trim().is_empty() {
                    bail!("--server requires a non-empty value");
                }
                out.server = Some(value);
                args.remove(i);
                continue;
            }
            if let Some(value) = strip_eq_prefix(arg, "--token") {
                if value.is_empty() {
                    bail!("--token requires a value");
                }
                out.token = Some(value.to_string());
                args.remove(i);
                continue;
            }
            if arg == "--token" {
                if i + 1 >= args.len() {
                    bail!("--token requires a value");
                }
                let value = args.remove(i + 1);
                if value.trim().is_empty() {
                    bail!("--token requires a non-empty value");
                }
                out.token = Some(value);
                args.remove(i);
                continue;
            }
            i += 1;
        }
        Ok(out)
    }

    /// Returns `Some(trigger_label)` if HTTP mode was requested via any of:
    /// `--http`, `--server`, `--token`, or `SYSLOG_USE_HTTP=1|true`. Returns
    /// `None` for the default Local mode. The label is the literal flag the
    /// user passed, used verbatim in error messages.
    ///
    /// Note: `SYSLOG_API_TOKEN` being set does NOT trigger HTTP — only the
    /// explicit opt-ins above do (locked decision).
    pub(crate) fn http_trigger(&self) -> Option<&'static str> {
        if let Some(flag) = self.http_flag_trigger() {
            return Some(flag);
        }
        if env_opts_into_http() {
            return Some("SYSLOG_USE_HTTP=1");
        }
        None
    }

    /// Like [`http_trigger`] but only considers explicit command-line FLAGS,
    /// ignoring the `SYSLOG_USE_HTTP` env var. Used by local-only commands
    /// (`compose`, `setup`) that must not bail just because operators have
    /// `SYSLOG_USE_HTTP=true` written into `~/.syslog-mcp/.env`.
    pub(crate) fn http_flag_trigger(&self) -> Option<&'static str> {
        if self.force_http {
            return Some("--http");
        }
        if self.server.is_some() {
            return Some("--server");
        }
        if self.token.is_some() {
            return Some("--token");
        }
        None
    }

    /// Build an [`HttpClient`] from these flags. On discovery failure, wraps
    /// the underlying error with a prefix naming the trigger so the operator
    /// knows exactly which knob put them into HTTP mode — this is the
    /// fail-closed contract from eng-review #C6.
    pub(crate) fn build_http_client(
        &self,
        trigger: &'static str,
    ) -> Result<super::http_client::HttpClient> {
        super::http_client::HttpClient::discover(self.server.clone(), self.token.clone())
            .map_err(|err| anyhow!("HTTP mode requested via {trigger} but discovery failed: {err}"))
    }
}

/// Returns `true` when `SYSLOG_USE_HTTP` is set to `1` or `true`
/// (case-insensitive). Any other value — including empty string, `0`, `false`,
/// or typos — is treated as unset.
fn env_opts_into_http() -> bool {
    match std::env::var(ENV_USE_HTTP) {
        Ok(v) => {
            let v = v.trim();
            v.eq_ignore_ascii_case("1") || v.eq_ignore_ascii_case("true")
        }
        Err(_) => false,
    }
}

/// If `arg` matches `flag=...` return the suffix; otherwise `None`.
pub(super) fn strip_eq_prefix<'a>(arg: &'a str, flag: &str) -> Option<&'a str> {
    arg.strip_prefix(flag)
        .and_then(|rest| rest.strip_prefix('='))
}
