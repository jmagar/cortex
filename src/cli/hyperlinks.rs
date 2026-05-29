//! OSC 8 hyperlink emitter. Modern terminals (kitty, iTerm2, wezterm, vscode,
//! Windows Terminal, gnome-terminal 3.26+) render the label as a clickable link.
//! Unsupported terminals print the label as plain text.
//!
//! Format: `\x1b]8;;URL\x1b\\TEXT\x1b]8;;\x1b\\`
#![allow(dead_code)]

use super::color::color_enabled;

#[cfg(test)]
#[path = "hyperlinks_tests.rs"]
mod tests;

const OSC8: &str = "\x1b]8;;";
const ST: &str = "\x1b\\";

/// Render `label` as a clickable link to `url` if the terminal supports OSC 8
/// and color is enabled. Otherwise return `label` (or `url` when label is empty).
pub(crate) fn hyperlink(url: &str, label: &str) -> String {
    let supported = color_enabled() && supports_hyperlinks::on(supports_hyperlinks::Stream::Stdout);
    hyperlink_inner(url, label, supported)
}

pub(crate) fn hyperlink_inner(url: &str, label: &str, supported: bool) -> String {
    let clean_url = strip_controls(url);
    let clean_label = strip_controls(label);
    let visible = if clean_label.is_empty() {
        clean_url.as_str()
    } else {
        clean_label.as_str()
    };
    if !supported {
        return visible.to_string();
    }
    format!("{OSC8}{clean_url}{ST}{visible}{OSC8}{ST}")
}

fn strip_controls(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_control()).collect()
}
