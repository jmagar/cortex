//! Unicode sparkline renderer — one char per data point at 8 levels.

use super::color::{ansi_colorize, color_enabled, CYAN_ANSI};

#[cfg(test)]
#[path = "sparkline_tests.rs"]
mod tests;

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Render `values` as a sparkline. Empty input → empty string.
/// Returns Aurora cyan text when color is enabled.
pub(crate) fn sparkline(values: &[u64]) -> String {
    if color_enabled() {
        ansi_colorize(CYAN_ANSI, &sparkline_plain(values))
    } else {
        sparkline_plain(values)
    }
}

pub(crate) fn sparkline_plain(values: &[u64]) -> String {
    if values.is_empty() {
        return String::new();
    }
    let min = *values.iter().min().unwrap();
    let max = *values.iter().max().unwrap();
    if min == max {
        return BLOCKS[3].to_string().repeat(values.len());
    }
    let range = (max - min) as f64;
    values
        .iter()
        .map(|&v| {
            let normalized = ((v - min) as f64) / range;
            let idx =
                ((normalized * (BLOCKS.len() - 1) as f64).round() as usize).min(BLOCKS.len() - 1);
            BLOCKS[idx]
        })
        .collect()
}
