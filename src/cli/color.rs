use std::env;
use std::io::IsTerminal;
use std::sync::atomic::AtomicU8;

// Aurora design-system CLI tokens (dark-first, operator-grade palette).
// Keep in sync with aurora-design-system/registry/aurora/styles/aurora.css
pub(crate) const PRIMARY_ANSI: &str = "\x1b[38;2;230;244;251m"; // #e6f4fb
pub(crate) const MUTED_ANSI: &str = "\x1b[38;2;167;188;201m"; //   #a7bcc9
pub(crate) const CYAN_ANSI: &str = "\x1b[38;2;41;182;246m"; //    #29b6f6
pub(crate) const SUCCESS_ANSI: &str = "\x1b[38;2;125;211;199m"; // #7dd3c7
pub(crate) const WARN_ANSI: &str = "\x1b[38;2;198;163;107m"; //   #c6a36b
pub(crate) const ERROR_ANSI: &str = "\x1b[38;2;199;132;144m"; //  #c78490
pub(crate) const VIOLET_ANSI: &str = "\x1b[38;2;167;139;250m"; // #a78bfa

#[allow(dead_code)]
const COLOR_AUTO: u8 = 0;
const COLOR_ALWAYS: u8 = 1;
const COLOR_NEVER: u8 = 2;

/// Set once at startup before any output is produced. Values: COLOR_AUTO / ALWAYS / NEVER.
///
/// Ordering: install_color_choice is called from the main thread before tokio
/// worker spawn; readers observe via the happens-before edge. Relaxed is safe
/// under that single-writer-before-readers contract.
pub(crate) static COLOR_OVERRIDE: AtomicU8 = AtomicU8::new(0);

/// Test-only mutex — sibling test modules that mutate COLOR_OVERRIDE must hold
/// this guard to prevent races under cargo test's parallel executor.
#[cfg(test)]
pub(crate) static COLOR_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ColorChoice {
    Auto,
    Always,
    Never,
}

/// Install the runtime color choice (called once at startup).
#[allow(dead_code)]
pub(crate) fn install_color_choice(choice: ColorChoice) {
    let val: u8 = match choice {
        ColorChoice::Auto => COLOR_AUTO,
        ColorChoice::Always => COLOR_ALWAYS,
        ColorChoice::Never => COLOR_NEVER,
    };
    COLOR_OVERRIDE.store(val, std::sync::atomic::Ordering::Relaxed);
}

/// Whether color should be emitted to stdout. Reads the runtime override plus
/// NO_COLOR / FORCE_COLOR / CLICOLOR_FORCE env vars, then TTY detection.
pub(crate) fn color_enabled() -> bool {
    color_enabled_for_tty(std::io::stdout().is_terminal())
}

#[allow(dead_code)]
pub(crate) fn color_forced_always() -> bool {
    COLOR_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) == COLOR_ALWAYS
}

#[allow(dead_code)]
pub(crate) fn color_forced_never() -> bool {
    COLOR_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) == COLOR_NEVER
}

fn color_env_forced() -> bool {
    env_flag("FORCE_COLOR") || env_flag("CLICOLOR_FORCE")
}

fn env_flag(var: &str) -> bool {
    env::var_os(var)
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

fn color_enabled_for_tty(is_terminal: bool) -> bool {
    match COLOR_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) {
        COLOR_ALWAYS => true,
        COLOR_NEVER => false,
        _ => {
            if env::var_os("NO_COLOR").is_some() {
                return false;
            }
            if color_env_forced() {
                return true;
            }
            is_terminal
        }
    }
}

pub(crate) fn ansi_colorize(code: &str, text: &str) -> String {
    if color_enabled() {
        format!("{code}{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

// ── Free color helpers — use these instead of Palette ──────────────────────

pub(crate) fn primary(text: &str) -> String {
    ansi_colorize(PRIMARY_ANSI, text)
}

pub(crate) fn muted(text: &str) -> String {
    ansi_colorize(MUTED_ANSI, text)
}

pub(crate) fn cyan(text: &str) -> String {
    ansi_colorize(CYAN_ANSI, text)
}

pub(crate) fn success(text: &str) -> String {
    ansi_colorize(SUCCESS_ANSI, text)
}

pub(crate) fn warn(text: &str) -> String {
    ansi_colorize(WARN_ANSI, text)
}

pub(crate) fn error(text: &str) -> String {
    ansi_colorize(ERROR_ANSI, text)
}

pub(crate) fn violet(text: &str) -> String {
    ansi_colorize(VIOLET_ANSI, text)
}

pub(crate) fn severity(sev: &str) -> String {
    if !color_enabled() {
        return sev.to_string();
    }
    let lower = sev.to_ascii_lowercase();
    if lower.starts_with("err") || lower == "crit" || lower == "alert" || lower == "emerg" {
        ansi_colorize(ERROR_ANSI, sev)
    } else if lower.starts_with("warn") {
        ansi_colorize(WARN_ANSI, sev)
    } else if lower == "info" || lower == "notice" {
        ansi_colorize(SUCCESS_ANSI, sev)
    } else {
        ansi_colorize(MUTED_ANSI, sev)
    }
}

/// `"42 logs"` — both parts colored cyan.
#[allow(dead_code)]
pub(crate) fn metric(value: impl std::fmt::Display, label: &str) -> String {
    format!("{} {}", cyan(&value.to_string()), cyan(label))
}

/// `"error: <msg>"` on stderr in Aurora rose-red.
#[allow(dead_code)]
pub(crate) fn report_error(msg: &str) {
    eprintln!("{} {}", error("error:"), msg);
}

/// `"hint: <msg>"` on stderr in Aurora cyan — companion to report_error.
#[allow(dead_code)]
pub(crate) fn report_hint(msg: &str) {
    eprintln!("{} {}", cyan("hint:"), msg);
}

#[allow(dead_code)]
pub(crate) fn symbol_for_status(status: &str) -> String {
    match status {
        "ok" | "completed" => success("✓"),
        "failed" | "error" => error("✗"),
        "running" | "processing" => cyan("◐"),
        "warn" | "warning" => warn("⚠"),
        _ => cyan("•"),
    }
}

#[allow(dead_code)]
pub(crate) fn status_text(status: &str) -> String {
    match status {
        "ok" | "completed" => success(status),
        "failed" | "error" => error(status),
        "running" | "processing" => cyan(status),
        "warn" | "warning" => warn(status),
        _ => cyan(status),
    }
}
