//! Aurora bordered summary panel. Use for terminal "done" output:
//! db status, doctor summary, ai-watch overview.
//!
//! ```text
//! ╭─ DB Status ──────╮
//! │ db_path  /data   │
//! │ pages    42      │
//! ╰──────────────────╯
//! ```
#![allow(dead_code)]

use super::color::{CYAN_ANSI, MUTED_ANSI, PRIMARY_ANSI, ansi_colorize, color_enabled};

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;

/// Render a titled key/value panel. Honors NO_COLOR / TTY detection.
pub(crate) fn panel(title: &str, rows: &[(&str, &str)]) -> String {
    render(title, rows, color_enabled())
}

#[cfg(test)]
pub(crate) fn panel_plain(title: &str, rows: &[(&str, &str)]) -> String {
    render(title, rows, false)
}

fn render(title: &str, rows: &[(&str, &str)], color: bool) -> String {
    let title_chars = title.chars().count();
    let key_w = rows
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    let val_w = rows
        .iter()
        .map(|(_, v)| v.chars().count())
        .max()
        .unwrap_or(0);
    // body visible width: 1 space + key_w + 2 sep + val_w + 1 space
    let row_visible = if rows.is_empty() {
        0
    } else {
        1 + key_w + 2 + val_w + 1
    };
    // top visible width must be ≥ title_chars + 3 (─ space title space)
    let inner_w = row_visible.max(title_chars + 3);
    let dashes_after_title = inner_w - title_chars - 3;
    let extra_row_pad = inner_w.saturating_sub(row_visible);

    let border = |s: &str| -> String {
        if color {
            ansi_colorize(CYAN_ANSI, s)
        } else {
            s.to_string()
        }
    };
    let title_styled = if color {
        ansi_colorize(PRIMARY_ANSI, title)
    } else {
        title.to_string()
    };
    let key_styled = |k: &str| -> String {
        if color {
            ansi_colorize(MUTED_ANSI, k)
        } else {
            k.to_string()
        }
    };
    let dashes = |n: usize| -> String { border("─").repeat(n) };

    let mut out = String::new();

    // Top: ╭─ title ─...─╮
    out.push_str(&border("╭"));
    out.push_str(&border("─"));
    out.push(' ');
    out.push_str(&title_styled);
    out.push(' ');
    out.push_str(&dashes(dashes_after_title));
    out.push_str(&border("╮"));
    out.push('\n');

    // Rows: │ key  value <pad>│
    for (k, v) in rows {
        out.push_str(&border("│"));
        out.push(' ');
        out.push_str(&key_styled(k));
        out.push_str(&" ".repeat(key_w - k.chars().count()));
        out.push_str("  ");
        out.push_str(v);
        out.push_str(&" ".repeat(val_w - v.chars().count()));
        out.push(' ');
        out.push_str(&" ".repeat(extra_row_pad));
        out.push_str(&border("│"));
        out.push('\n');
    }

    // Bottom: ╰───╯
    out.push_str(&border("╰"));
    out.push_str(&dashes(inner_w));
    out.push_str(&border("╯"));
    out
}
