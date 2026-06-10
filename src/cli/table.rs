//! Aurora-themed table renderer. Wraps `comfy-table` with rounded UTF-8
//! borders, cyan headers, and a `--color=never`-friendly ASCII fallback.
#![allow(dead_code)]

use comfy_table::{Cell, Color, ContentArrangement, Table, modifiers, presets};

use super::color::color_enabled;

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;

/// Build a table pre-styled with Aurora colors. Caller adds rows.
///
/// ```ignore
/// let mut t = aurora_table(&["HOST", "COUNT", "LAST SEEN"]);
/// t.add_row(vec!["myhost".to_string(), "42".to_string(), "1m ago".to_string()]);
/// println!("{t}");
/// ```
pub(crate) fn aurora_table(headers: &[&str]) -> Table {
    let mut t = Table::new();
    if color_enabled() {
        t.load_preset(presets::UTF8_FULL)
            .apply_modifier(modifiers::UTF8_ROUND_CORNERS)
            .set_content_arrangement(ContentArrangement::Dynamic);
        let cyan = Color::Rgb {
            r: 41,
            g: 182,
            b: 246,
        };
        t.set_header(headers.iter().map(|h| Cell::new(h).fg(cyan)));
    } else {
        t.load_preset(presets::ASCII_FULL_CONDENSED);
        t.set_header(headers.to_vec());
    }
    t
}

pub(crate) fn print_aurora_table(headers: &[&str], rows: impl IntoIterator<Item = Vec<String>>) {
    let mut t = aurora_table(headers);
    for row in rows {
        t.add_row(row);
    }
    println!("{t}");
}
