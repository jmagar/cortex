use chrono::Utc;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::inventory::collectors::CollectorOutput;
use crate::inventory::http::{HttpProbe, api_key_header};
use crate::inventory::schema::{InventoryNode, Provenance, StorageSummary, TrustLevel};

const SECTIONS: &[(&str, &str)] = &[
    ("system", "{ info { host os version machineId } }"),
    ("array", "{ array { state disks { name status size } } }"),
];

pub async fn collect(
    url: Option<&str>,
    api_key: Option<&str>,
    timeout: Duration,
) -> CollectorOutput {
    let mut out = CollectorOutput::new("unraid");
    let (Some(url), Some(api_key)) = (url, api_key) else {
        out.warn(
            "config",
            "CORTEX_UNRAID_URL/API_KEY not set; Unraid collection skipped",
        );
        return out;
    };
    let Ok(http) = HttpProbe::new(timeout) else {
        out.warn("http", "failed to initialize Unraid HTTP client");
        return out;
    };
    let headers = match api_key_header("x-api-key", api_key) {
        Ok(headers) => headers,
        Err(error) => {
            out.warn(
                "config",
                format!("Unraid API key contains invalid header characters: {error}"),
            );
            return out;
        }
    };
    let endpoint = format!("{}/graphql", url.trim_end_matches('/'));
    for (section, query) in SECTIONS {
        match http
            .post_json(&endpoint, headers.clone(), json!({ "query": query }))
            .await
        {
            Ok(response) if response.status < 400 => {
                normalize_section(url, section, &response.body, &mut out)
            }
            Ok(response) => out.warn(
                section,
                format!("Unraid section {section} returned HTTP {}", response.status),
            ),
            Err(error) => out.warn(section, format!("Unraid section {section} failed: {error}")),
        }
    }
    out
}

fn normalize_section(url: &str, section: &str, body: &Value, out: &mut CollectorOutput) {
    if let Some(errors) = body.get("errors").and_then(Value::as_array) {
        out.warn(
            section,
            format!("Unraid GraphQL {section} returned {} errors", errors.len()),
        );
    }
    if section == "system" {
        let info = body.pointer("/data/info").unwrap_or(body);
        let hostname = info
            .get("host")
            .and_then(Value::as_str)
            .or_else(|| info.get("machineId").and_then(Value::as_str))
            .unwrap_or("unraid");
        out.nodes.push(InventoryNode {
            id: format!("unraid:{hostname}"),
            hostname: hostname.to_string(),
            trust_level: TrustLevel::Observed,
            provenance: provenance(url, section),
            roles: vec!["unraid".to_string()],
            ips: Vec::new(),
            os: info
                .get("os")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            cpu: None,
            memory: None,
            listeners: Vec::new(),
            storage: Vec::new(),
            extras: BTreeMap::new(),
        });
    }
    if section == "array" {
        for disk in body
            .pointer("/data/array/disks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = disk.get("name").and_then(Value::as_str).unwrap_or("disk");
            out.storage.push(StorageSummary {
                id: format!("unraid:{name}"),
                mount: first_string(disk, &["mountpoint", "mount"])
                    .unwrap_or_else(|| name.to_string()),
                fs_type: first_string(disk, &["filesystem", "fstype", "fsType", "status"]),
                total_bytes: parse_u64_value(disk.get("size")),
                available_bytes: None,
                provenance: provenance(url, section),
            });
        }
    }
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(ToString::to_string)
}

fn parse_u64_value(value: Option<&Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
    })
}

fn provenance(url: &str, section: &str) -> Provenance {
    Provenance::new(
        format!("{}/graphql#{section}", url.trim_end_matches('/')),
        "source_inventory",
        Utc::now().to_rfc3339(),
    )
}

#[cfg(test)]
#[path = "unraid_tests.rs"]
mod tests;
