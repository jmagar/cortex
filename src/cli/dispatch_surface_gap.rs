use super::dispatch::http_or_cancel;
use super::output_common::print_json;
use super::output_graph::{
    print_graph_around_response, print_graph_entity_lookup_response,
    print_graph_evidence_lookup_response, print_graph_rebuild_response,
    print_graph_status_response,
};

use anyhow::Result;
use cortex::app::{
    CorrelateStateRequest, FleetStateRequest, GraphAroundRequest, GraphEntityLookupRequest,
    GraphEvidenceLookupRequest, GraphExplainRequest, HostStateRequest, StateRequest, StateResponse,
    TopicCorrelateRequest,
};

use super::CliMode;
use super::args::{
    CorrelateStateArgs, EntityArgs, FleetStateArgs, GraphAroundArgs, GraphEvidenceArgs,
    GraphExplainArgs, GraphRebuildArgs, GraphStatusArgs, HostStateArgs, TopicCorrelateArgs,
};

// ─── Heartbeat fleet state (cxih.4) ─────────────────────────────────────────

impl HostStateArgs {
    pub(crate) fn into_request(self) -> HostStateRequest {
        HostStateRequest {
            host_id: self.host_id,
            host: self.host,
            since: self.since,
            limit: self.limit,
        }
    }
}

impl FleetStateArgs {
    pub(crate) fn into_request(self) -> FleetStateRequest {
        FleetStateRequest {
            include_ok: self.include_ok,
            sort: self.sort,
        }
    }
}

impl CorrelateStateArgs {
    pub(crate) fn into_request(self) -> Result<CorrelateStateRequest> {
        Ok(CorrelateStateRequest {
            reference_time: self
                .reference_time
                .ok_or_else(|| anyhow::anyhow!("--reference-time is required"))?,
            window_minutes: self.window_minutes,
            host: self.host,
            severity_min: self.severity_min,
            limit: self.limit,
        })
    }
}

impl EntityArgs {
    pub(crate) fn into_request(self) -> GraphEntityLookupRequest {
        GraphEntityLookupRequest {
            mode: Some("entity".into()),
            entity_id: None,
            entity_type: self.entity_type,
            key: self.key,
            alias_type: self.alias_type,
            alias_key: self.alias_key,
            limit: self.limit,
            evidence_sample_limit: self.evidence_sample_limit,
            payload_budget: self.payload_budget,
        }
    }
}

impl GraphAroundArgs {
    pub(crate) fn into_request(self) -> GraphAroundRequest {
        GraphAroundRequest {
            mode: Some("around".into()),
            entity_id: self.entity_id,
            entity_type: self.entity_type,
            key: self.key,
            alias_type: self.alias_type,
            alias_key: self.alias_key,
            depth: self.depth,
            limit: self.limit,
            evidence_sample_limit: self.evidence_sample_limit,
            payload_budget: self.payload_budget,
        }
    }
}

impl GraphExplainArgs {
    pub(crate) fn into_request(self) -> GraphExplainRequest {
        GraphExplainRequest {
            mode: Some("explain".into()),
            entity_id: self.entity_id,
            entity_type: self.entity_type,
            key: self.key,
            alias_type: self.alias_type,
            alias_key: self.alias_key,
            depth: self.depth,
            beam_width: self.beam_width,
            max_chains: self.max_chains,
            evidence_sample_limit: self.evidence_sample_limit,
            payload_budget: self.payload_budget,
        }
    }
}

impl GraphEvidenceArgs {
    pub(crate) fn into_request(self) -> GraphEvidenceLookupRequest {
        GraphEvidenceLookupRequest {
            mode: Some("evidence".into()),
            evidence_id: self.evidence_id,
            payload_budget: self.payload_budget,
        }
    }
}

pub(crate) async fn run_host_state(mode: &CliMode, args: HostStateArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => match service.state(StateRequest::Host(req)).await? {
            StateResponse::Host(response) => *response,
            StateResponse::Fleet(_) | StateResponse::ClockSkew(_) => {
                anyhow::bail!("internal: state host returned wrong response")
            }
        },
        CliMode::Http(client) => http_or_cancel(client.host_state(&req)).await?,
    };
    if json {
        return print_json(&response);
    }
    println!(
        "host_id={} hostname={} samples={}{}",
        response.host_id,
        response.hostname,
        response.total_samples,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        },
    );
    let f = &response.flags;
    println!(
        "flags: partial={} late={} clock_skew={} cpu={} mem={} swap={} disk={} net_err={} container_unhealthy={}",
        f.collector_partial,
        f.heartbeat_late,
        f.clock_skew,
        f.cpu_pressure,
        f.memory_pressure,
        f.swap_pressure,
        f.disk_capacity_pressure,
        f.network_error_pressure,
        f.container_unhealthy,
    );
    if let Some(latest) = &response.latest {
        println!(
            "latest: sampled_at={} seq={} uptime_secs={} agent={}",
            latest.sampled_at, latest.sequence, latest.uptime_secs, latest.agent_version
        );
    }
    Ok(())
}

pub(crate) async fn run_fleet_state(mode: &CliMode, args: FleetStateArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => match service.state(StateRequest::Fleet(req)).await? {
            StateResponse::Fleet(response) => response,
            StateResponse::Host(_) | StateResponse::ClockSkew(_) => {
                anyhow::bail!("internal: state fleet returned wrong response")
            }
        },
        CliMode::Http(client) => http_or_cancel(client.fleet_state(&req)).await?,
    };
    if json {
        return print_json(&response);
    }
    let s = &response.summary;
    println!(
        "{} host(s): ok={} late={} partial={} pressure={}",
        s.total, s.ok, s.late, s.partial, s.pressure
    );
    for h in &response.hosts {
        let pressure = if h.pressure.is_empty() {
            "-".to_string()
        } else {
            h.pressure.join(",")
        };
        println!(
            "  {:<20} status={:<8} last_heartbeat={} pressure={}",
            h.hostname, h.status, h.last_heartbeat_at, pressure
        );
    }
    Ok(())
}

pub(crate) async fn run_correlate_state(mode: &CliMode, args: CorrelateStateArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request()?;
    let response = match mode {
        CliMode::Local(service) => service.correlate_state(req).await?,
        CliMode::Http(client) => http_or_cancel(client.correlate_state(&req)).await?,
    };
    if json {
        return print_json(&response);
    }
    println!(
        "window {} → {}{}",
        response.window.from,
        response.window.to,
        if response.truncated {
            " (truncated)"
        } else {
            ""
        },
    );
    println!("{} host(s):", response.hosts.len());
    for h in &response.hosts {
        let summary = &h.heartbeat_summary;
        println!(
            "  {:<20} heartbeats={} partial={} logs={}",
            h.hostname,
            summary.samples,
            summary.partial_samples,
            h.logs.len()
        );
    }
    Ok(())
}

impl TopicCorrelateArgs {
    pub(crate) fn into_request(self) -> Result<TopicCorrelateRequest> {
        let topic = self
            .topic
            .ok_or_else(|| anyhow::anyhow!("a topic is required"))?;
        Ok(TopicCorrelateRequest {
            topic,
            since: self.since,
            until: self.until,
            depth: self.depth,
            source_kinds: self.source_kinds,
            limit: self.limit,
        })
    }
}

pub(crate) async fn run_topic_correlate(mode: &CliMode, args: TopicCorrelateArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request()?;
    let response = match mode {
        CliMode::Local(service) => service.topic_correlate(req).await?,
        CliMode::Http(client) => http_or_cancel(client.topic_correlate(&req)).await?,
    };
    if json {
        return print_json(&response);
    }
    println!(
        "topic '{}' → {} entit{} resolved, {} expanded, {} timeline row(s){}",
        response.topic,
        response.resolved_entities.len(),
        if response.resolved_entities.len() == 1 {
            "y"
        } else {
            "ies"
        },
        response.graph_expansion.len(),
        response.timeline.len(),
        if response.truncated {
            " (truncated)"
        } else {
            ""
        },
    );
    for entity in &response.resolved_entities {
        println!(
            "  resolved {}:{} ({})",
            entity.entity_type, entity.key, entity.match_kind
        );
    }
    if !response.discovered_hosts.is_empty() {
        println!("  hosts: {}", response.discovered_hosts.join(", "));
    }
    for row in &response.timeline {
        println!(
            "  {}  [{}]  {}  {}",
            row.timestamp,
            row.entity_path,
            row.hostname,
            row.message.chars().take(120).collect::<String>(),
        );
    }
    Ok(())
}

pub(crate) async fn run_entity_lookup(mode: &CliMode, args: EntityArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.graph_entity_lookup(req).await?,
        CliMode::Http(client) => http_or_cancel(client.graph_entity(&req)).await?,
    };
    print_graph_entity_lookup_response(&response, json)
}

pub(crate) async fn run_graph_around(mode: &CliMode, args: GraphAroundArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.graph_around(req).await?,
        CliMode::Http(client) => http_or_cancel(client.graph_around(&req)).await?,
    };
    print_graph_around_response(&response, json)
}

pub(crate) async fn run_graph_explain(mode: &CliMode, args: GraphExplainArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.graph_explain(req).await?,
        CliMode::Http(client) => http_or_cancel(client.graph_explain(&req)).await?,
    };
    super::output_graph::print_graph_explain_response(&response, json)
}

pub(crate) async fn run_graph_evidence(mode: &CliMode, args: GraphEvidenceArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.graph_evidence_lookup(req).await?,
        CliMode::Http(client) => http_or_cancel(client.graph_evidence(&req)).await?,
    };
    print_graph_evidence_lookup_response(&response, json)
}

pub(crate) async fn run_graph_status(mode: &CliMode, args: GraphStatusArgs) -> Result<()> {
    let response = match mode {
        CliMode::Local(service) => service.graph_projection_status().await?,
        CliMode::Http(_) => {
            anyhow::bail!("graph status is local-only; run it on the host with DB access")
        }
    };
    print_graph_status_response(&response, args.json)
}

pub(crate) async fn run_graph_rebuild(mode: &CliMode, args: GraphRebuildArgs) -> Result<()> {
    let response = match mode {
        CliMode::Local(service) => service.graph_rebuild().await?,
        CliMode::Http(_) => {
            anyhow::bail!("graph rebuild is local-only; run it on the host with DB access")
        }
    };
    print_graph_rebuild_response(&response, args.json)
}

#[cfg(test)]
#[path = "dispatch_surface_gap_tests.rs"]
mod tests;
