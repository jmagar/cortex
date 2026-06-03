use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

use crate::inventory::collectors::CollectorOutput;
use crate::inventory::process::run_command;
use crate::inventory::schema::{InventoryNode, Provenance, TrustLevel};

pub async fn collect(timeout: Duration) -> CollectorOutput {
    let mut out = CollectorOutput::new("tailscale");
    match run_command("tailscale", &["status", "--json"], timeout).await {
        Ok(output) if output.status == Some(0) => parse_status(&output.stdout, &mut out),
        Ok(output) => out.warn(
            "status",
            format!("tailscale status failed: {}", output.stderr),
        ),
        Err(error) => out.warn("status", format!("tailscale CLI unavailable: {error}")),
    }
    out
}

fn parse_status(body: &str, out: &mut CollectorOutput) {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        out.warn("status", "tailscale status JSON was invalid");
        return;
    };
    let now = Utc::now().to_rfc3339();
    let self_node = value.get("Self");
    if let Some(hostname) = self_node
        .and_then(|node| node.get("HostName"))
        .and_then(Value::as_str)
    {
        let ips = self_node
            .and_then(|node| node.get("TailscaleIPs"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect();
        out.nodes.push(InventoryNode {
            id: format!("tailscale:{hostname}"),
            hostname: hostname.to_string(),
            trust_level: TrustLevel::Verified,
            provenance: Provenance::new("tailscale status --json", "source_inventory", now),
            roles: vec!["tailscale_peer".to_string()],
            ips,
            os: self_node
                .and_then(|n| n.get("OS"))
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

#[cfg(test)]
#[path = "tailscale_tests.rs"]
mod tests;
