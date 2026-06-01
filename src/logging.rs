//! Logging initialization — Aurora console formatter + JSON file layer.
//!
//! Aurora color palette for console output is defined in `aurora.rs`.
//! Colors match `lab/crates/lab/src/output/theme.rs` exactly.

pub mod aurora;

use std::io::IsTerminal;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialize tracing with:
/// - Console layer: Aurora-colored output to stderr (color auto-detected)
/// - File layer: JSON to `{data_dir}/logs/cortex.log` (if writable)
///
/// Returns a string describing the active log filter for startup log output.
pub fn init(default_filter: &str) -> String {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    let filter_str = filter.to_string();

    let colorize = should_colorize();

    let console = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(colorize)
        .with_target(false)
        .event_format(AuroraLevelFormatter);

    tracing_subscriber::registry()
        .with(console.with_filter(filter))
        .init();

    filter_str
}

/// Returns whether stderr should emit ANSI escape codes.
///
/// Delegates to the unified [`crate::color_policy`] so the `--color` override
/// (set once at startup) plus `NO_COLOR` / `FORCE_COLOR` / `CLICOLOR_FORCE` and
/// TTY detection govern the tracing console the same as every other surface.
pub fn should_colorize() -> bool {
    crate::color_policy::resolve(std::io::stderr().is_terminal())
}

// ── Minimal level-only formatter ─────────────────────────────────────────────
//
// Uses aurora ANSI 256 level colors while keeping the rest of the default
// tracing_subscriber format (timestamp, target, fields).

use std::fmt as stdfmt;
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::{
    format::{FormatEvent, FormatFields, Writer},
    FmtContext,
};
use tracing_subscriber::registry::LookupSpan;

struct AuroraLevelFormatter;

impl<S, N> FormatEvent<S, N> for AuroraLevelFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> stdfmt::Result {
        let ansi = writer.has_ansi_escapes();
        let level = *event.metadata().level();

        // Timestamp (HH:MM:SS local)
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        if ansi {
            write!(writer, "{}  ", aurora::dim(&ts))?;
        } else {
            write!(writer, "{ts}  ")?;
        }

        // Level — Aurora colors
        let level_str = if ansi {
            match level {
                tracing::Level::ERROR => aurora::bold(aurora::ERROR, "ERROR"),
                tracing::Level::WARN => aurora::bold(aurora::WARN, " WARN"),
                tracing::Level::INFO => " INFO".to_string(),
                tracing::Level::DEBUG => aurora::dim("DEBUG"),
                tracing::Level::TRACE => aurora::dim("TRACE"),
            }
        } else {
            match level {
                tracing::Level::ERROR => "ERROR".to_string(),
                tracing::Level::WARN => " WARN".to_string(),
                tracing::Level::INFO => " INFO".to_string(),
                tracing::Level::DEBUG => "DEBUG".to_string(),
                tracing::Level::TRACE => "TRACE".to_string(),
            }
        };
        write!(writer, "{level_str}  ")?;

        // Message + fields (delegated to the default field formatter)
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}
