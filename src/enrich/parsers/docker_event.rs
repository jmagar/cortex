//! Docker lifecycle event parser. Spec §7.2.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::enrich::{Parser, ParserError, ParserInput, ParserOutput};

pub struct DockerEventParser;

static EVENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^docker container event:\s+(?P<action>\S+)\s+(?P<attrs>.*)").expect("static regex")
});

static ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)=([^\s]+)").expect("static regex"));

impl Parser for DockerEventParser {
    fn name(&self) -> &'static str {
        "docker_event"
    }

    fn namespace(&self) -> &'static str {
        "docker"
    }

    fn parse(&self, input: ParserInput<'_>) -> Result<ParserOutput, ParserError> {
        let caps = EVENT_RE
            .captures(input.message)
            .ok_or(ParserError::NoMatch("not a docker event line"))?;
        let action = caps["action"].to_string();
        let mut metadata = Map::new();
        for m in ATTR_RE.captures_iter(&caps["attrs"]) {
            let key = m[1].to_string();
            let val = m[2].to_string();
            if key == "exit_code" {
                if let Ok(n) = val.parse::<i32>() {
                    metadata.insert(key, json!(n));
                    continue;
                }
            }
            metadata.insert(key, Value::String(val));
        }
        // Hoist `container` attr to canonical key `container_name`.
        if let Some(Value::String(s)) = metadata.remove("container") {
            metadata.insert("container_name".to_string(), Value::String(s));
        }
        let severity = match action.as_str() {
            "oom" => Some("crit"),
            "die" | "kill" | "health_status_unhealthy" => Some("warning"),
            _ => None,
        };
        Ok(ParserOutput {
            event_action: Some(action),
            severity,
            metadata,
            ..Default::default()
        })
    }
}

#[cfg(test)]
#[path = "docker_event_tests.rs"]
mod docker_event_tests;
