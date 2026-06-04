use std::collections::BTreeMap;
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;

use crate::inventory::collectors::CollectorOutput;
use crate::inventory::process::run_command;
use crate::inventory::schema::{
    InventoryNode, ListenerFact, Provenance, StorageSummary, TrustLevel,
};

pub async fn collect(timeout: Duration) -> CollectorOutput {
    let mut out = CollectorOutput::new("device");
    let now = Utc::now().to_rfc3339();
    let hostname = command_stdout("hostname", &[], timeout)
        .await
        .unwrap_or_else(|| "localhost".into());
    let os = command_stdout("uname", &["-srvmo"], timeout).await;
    let memory = parse_meminfo();
    let ips = collect_ips(timeout, &mut out).await;
    let listeners = collect_listeners(timeout, &mut out).await;
    let storage = collect_storage(timeout, &mut out).await;

    out.nodes.push(InventoryNode {
        id: format!("host:{hostname}"),
        hostname,
        trust_level: TrustLevel::Observed,
        provenance: Provenance::new("local host probes", "source_inventory", now),
        roles: vec!["cortex_collector".to_string()],
        ips,
        os,
        cpu: cpu_model(),
        memory,
        listeners,
        storage: storage.clone(),
        extras: BTreeMap::new(),
    });
    out.storage.extend(storage);
    out
}

async fn command_stdout(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    run_command(program, args, timeout)
        .await
        .ok()
        .filter(|output| output.status == Some(0))
        .map(|output| output.stdout.trim().to_string())
        .filter(|stdout| !stdout.is_empty())
}

async fn collect_ips(timeout: Duration, out: &mut CollectorOutput) -> Vec<String> {
    match command_stdout("ip", &["-j", "addr"], timeout).await {
        Some(body) => serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| value.as_array().cloned())
            .map(|interfaces| {
                interfaces
                    .iter()
                    .flat_map(|iface| {
                        iface
                            .get("addr_info")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                    })
                    .filter_map(|addr| addr.get("local").and_then(Value::as_str))
                    .filter(|ip| *ip != "127.0.0.1" && *ip != "::1")
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        None => {
            out.warn("ips", "ip -j addr unavailable");
            Vec::new()
        }
    }
}

async fn collect_listeners(timeout: Duration, out: &mut CollectorOutput) -> Vec<ListenerFact> {
    let Some(body) = command_stdout("ss", &["-lntu"], timeout).await else {
        out.warn("listeners", "ss -lntu unavailable");
        return Vec::new();
    };
    body.lines()
        .skip(1)
        .filter_map(|line| {
            let cols = line.split_whitespace().collect::<Vec<_>>();
            let protocol = cols.first()?.to_ascii_lowercase();
            let bind = cols.get(4)?.to_string();
            let port = bind.rsplit(':').next().and_then(|part| part.parse().ok());
            Some(ListenerFact {
                protocol,
                bind,
                port,
                process: None,
            })
        })
        .collect()
}

async fn collect_storage(timeout: Duration, out: &mut CollectorOutput) -> Vec<StorageSummary> {
    let Some(body) = command_stdout("df", &["-PT"], timeout).await else {
        out.warn("storage", "df -PT unavailable");
        return Vec::new();
    };
    body.lines()
        .skip(1)
        .filter_map(|line| {
            let cols = line.split_whitespace().collect::<Vec<_>>();
            if cols.len() < 7 {
                return None;
            }
            let mount = cols[6].to_string();
            Some(StorageSummary {
                id: format!("storage:{mount}"),
                mount,
                fs_type: Some(cols[1].to_string()),
                total_bytes: cols[2].parse::<u64>().ok().map(|kb| kb * 1024),
                available_bytes: cols[4].parse::<u64>().ok().map(|kb| kb * 1024),
                provenance: Provenance::new("df -PT", "source_inventory", Utc::now().to_rfc3339()),
            })
        })
        .take(50)
        .collect()
}

fn cpu_model() -> Option<String> {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|body| {
            body.lines().find_map(|line| {
                line.strip_prefix("model name")
                    .and_then(|rest| rest.split_once(':'))
                    .map(|(_, v)| v.trim().to_string())
            })
        })
}

fn parse_meminfo() -> Option<String> {
    let body = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb = body
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())?;
    Some(format!("{} MiB", kb / 1024))
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;
