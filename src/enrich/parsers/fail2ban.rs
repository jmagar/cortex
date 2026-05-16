//! fail2ban parser. Spec §7.6.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use crate::enrich::{Parser, ParserError, ParserInput, ParserOutput};

pub struct Fail2banParser;

static LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"fail2ban\.\w+\s+\[\d+\]:\s+\w+\s+\[(?P<jail>[^\]]+)\]\s+(?P<verb>Restore Ban|Ban|Unban|Found)\s+(?P<ips>[\d\.:a-fA-F ]+?)(?:\s+-\s+\d|$)",
    )
    .expect("static regex")
});

impl Parser for Fail2banParser {
    fn name(&self) -> &'static str {
        "fail2ban"
    }

    fn namespace(&self) -> &'static str {
        "fail2ban"
    }

    fn parse(&self, input: ParserInput<'_>) -> Result<ParserOutput, ParserError> {
        let caps = LINE_RE
            .captures(input.message)
            .ok_or(ParserError::NoMatch("not a fail2ban action line"))?;

        let jail = caps["jail"].to_string();
        let verb = &caps["verb"];
        let ips: Vec<String> = caps["ips"]
            .split_whitespace()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if ips.is_empty() {
            return Err(ParserError::MissingField("banned_ip"));
        }

        let event_action = match verb {
            "Ban" => "ban",
            "Unban" => "unban",
            "Found" => "found",
            "Restore Ban" => "restore_ban",
            _ => unreachable!(),
        };

        let severity = match event_action {
            "ban" | "restore_ban" => Some("warning"),
            "unban" => Some("info"),
            "found" => Some("notice"),
            _ => None,
        };

        let mut metadata = Map::new();
        metadata.insert("jail".into(), Value::String(jail));
        metadata.insert("banned_ip".into(), Value::String(ips[0].clone()));
        if ips.len() > 1 {
            metadata.insert(
                "all_ips".into(),
                Value::Array(ips.iter().map(|s| Value::String(s.clone())).collect()),
            );
        }

        Ok(ParserOutput {
            event_action: Some(event_action.into()),
            severity,
            metadata,
            ..Default::default()
        })
    }
}

#[cfg(test)]
#[path = "fail2ban_tests.rs"]
mod fail2ban_tests;
