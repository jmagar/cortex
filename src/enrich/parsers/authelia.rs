//! Authelia auth parser. Spec §7.3.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::enrich::{AuthOutcome, Parser, ParserError, ParserInput, ParserOutput};

pub struct AutheliaParser;

static USERNAME_QUOTED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"user '([^']+)'").expect("static regex"));

impl Parser for AutheliaParser {
    fn name(&self) -> &'static str {
        "authelia"
    }

    fn namespace(&self) -> &'static str {
        "authelia"
    }

    fn parse(&self, input: ParserInput<'_>) -> Result<ParserOutput, ParserError> {
        let trimmed = input.message.trim_start();
        if !trimmed.starts_with('{') {
            return Err(ParserError::Structural("not json (text-mode authelia)"));
        }
        let value: Value = serde_json::from_str(trimmed)?;
        let obj = value
            .as_object()
            .ok_or(ParserError::Structural("json not object"))?;

        let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let msg = obj.get("msg").and_then(|v| v.as_str()).unwrap_or("");
        let level = obj.get("level").and_then(|v| v.as_str()).unwrap_or("info");
        let remote_ip = obj.get("remote_ip").and_then(|v| v.as_str());
        let method = obj.get("method").and_then(|v| v.as_str());
        let username = obj
            .get("username")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| USERNAME_QUOTED_RE.captures(msg).map(|c| c[1].to_string()));

        let severity = match level {
            "debug" => Some("debug"),
            "info" => Some("info"),
            "warning" | "warn" => Some("warning"),
            "error" => Some("err"),
            "critical" | "fatal" => Some("crit"),
            _ => None,
        };

        let is_auth_event = path.starts_with("/api/firstfactor")
            || path.starts_with("/api/secondfactor")
            || path.starts_with("/api/u2f")
            || path.starts_with("/api/duo");

        let auth_outcome = if is_auth_event {
            // Use case-insensitive matching and check unsuccessful BEFORE successful
            // to avoid "Unsuccessful" matching the "successful" branch first.
            let msg_lower = msg.to_ascii_lowercase();
            if msg_lower.contains("unsuccessful") {
                Some(AuthOutcome::Failure)
            } else if msg_lower.contains("successful") {
                Some(AuthOutcome::Success)
            } else if msg_lower.contains("denied") {
                Some(AuthOutcome::Denied)
            } else {
                None
            }
        } else {
            None
        };

        let mfa_method = if path.starts_with("/api/firstfactor") {
            Some("1fa")
        } else if path.contains("/secondfactor/totp") {
            Some("totp")
        } else if path.contains("/secondfactor/duo") || path.starts_with("/api/duo") {
            Some("duo")
        } else if path.contains("/secondfactor/webauthn") || path.starts_with("/api/u2f") {
            Some("webauthn")
        } else {
            None
        };

        let mut metadata = Map::new();
        if let Some(u) = username {
            metadata.insert("username".into(), Value::String(u));
        }
        if let Some(m) = mfa_method {
            metadata.insert("mfa_method".into(), json!(m));
        }
        if let Some(ip) = remote_ip {
            metadata.insert("src_ip".into(), json!(ip));
        }
        if let Some(m) = method {
            metadata.insert("method".into(), json!(m));
        }
        if !path.is_empty() {
            metadata.insert("path".into(), json!(path));
        }

        Ok(ParserOutput {
            auth_outcome,
            severity,
            metadata,
            ..Default::default()
        })
    }
}

#[cfg(test)]
#[path = "authelia_tests.rs"]
mod authelia_tests;
