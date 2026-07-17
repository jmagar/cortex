use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

use crate::inventory::collectors::CollectorOutput;
use crate::inventory::http::{HttpProbe, api_key_header};
use crate::inventory::schema::{InventoryNode, NetworkSegment, Provenance, TrustLevel};

pub async fn collect(
    url: Option<&str>,
    api_key: Option<&str>,
    timeout: Duration,
) -> CollectorOutput {
    let mut out = CollectorOutput::new("unifi");
    let (Some(url), Some(api_key)) = (url, api_key) else {
        out.skip("CORTEX_UNIFI_URL/API_KEY not set; UniFi collection skipped");
        return out;
    };
    let Ok(http) = HttpProbe::new(timeout) else {
        out.warn("http", "failed to initialize UniFi HTTP client");
        return out;
    };
    let headers = match api_key_header("x-api-key", api_key) {
        Ok(headers) => headers,
        Err(error) => {
            out.warn(
                "config",
                format!("UniFi API key contains invalid header characters: {error}"),
            );
            return out;
        }
    };
    for path in [
        "/proxy/network/integration/v1/sites",
        "/proxy/network/api/s/default/stat/device",
    ] {
        let endpoint = format!("{}{}", url.trim_end_matches('/'), path);
        match http.get_json(&endpoint, headers.clone()).await {
            Ok(response) if response.status < 400 => {
                normalize_unifi(url, path, &response.body, &mut out)
            }
            Ok(response) => out.warn(
                path,
                format!("UniFi endpoint {path} returned HTTP {}", response.status),
            ),
            Err(error) => out.warn(path, format!("UniFi endpoint {path} failed: {error}")),
        }
    }
    out
}

fn normalize_unifi(url: &str, path: &str, body: &Value, out: &mut CollectorOutput) {
    let items = body
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| body.as_array())
        .cloned()
        .unwrap_or_default();
    if items.len() > 200 {
        out.warn(
            path,
            format!(
                "UniFi endpoint {path} returned {} records; truncating to 200",
                items.len()
            ),
        );
        if let Some(error) = out.errors.last_mut() {
            error.truncated = true;
        }
    }
    for item in items.iter().take(200) {
        if path.contains("sites") {
            let name = item
                .get("name")
                .or_else(|| item.get("siteName"))
                .and_then(Value::as_str)
                .unwrap_or("site");
            out.networks.push(NetworkSegment {
                name: name.to_string(),
                kind: "unifi_site".to_string(),
                members: Vec::new(),
                provenance: provenance(url, path),
            });
        } else {
            let Some(hostname) = item
                .get("name")
                .or_else(|| item.get("hostname"))
                .or_else(|| item.get("mac"))
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
            else {
                out.warn(
                    path,
                    "UniFi device record missing name, hostname, mac, and id; skipped",
                );
                continue;
            };
            out.nodes.push(InventoryNode {
                id: format!("unifi:{hostname}"),
                hostname: hostname.to_string(),
                trust_level: TrustLevel::Observed,
                provenance: provenance(url, path),
                roles: vec!["network_device".to_string()],
                ips: item
                    .get("ip")
                    .and_then(Value::as_str)
                    .map(|ip| vec![ip.to_string()])
                    .unwrap_or_default(),
                os: item
                    .get("model")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                cpu: None,
                memory: None,
                listeners: Vec::new(),
                storage: Vec::new(),
                extras: BTreeMap::new(),
            });
        }
    }
}

fn provenance(url: &str, path: &str) -> Provenance {
    Provenance::new(
        format!("{}{}", url.trim_end_matches('/'), path),
        "source_inventory",
        Utc::now().to_rfc3339(),
    )
}

#[cfg(test)]
#[path = "unifi_tests.rs"]
mod tests;
