use super::dispatch::http_or_cancel;
use super::output_common::print_json;
use super::output_graph::{
    print_graph_around_response, print_graph_entity_lookup_response,
    print_graph_evidence_lookup_response, print_graph_rebuild_response,
    print_graph_status_response,
};

use anyhow::Result;
use cortex::app::{
    AnomaliesRequest, ClockSkewRequest, CompareRequest, CorrelateStateRequest, FleetStateRequest,
    GraphAroundRequest, GraphEntityLookupRequest, GraphEvidenceLookupRequest, GraphExplainRequest,
    HostStateRequest, ListAppsRequest, SilentHostsRequest,
};

use super::args::{
    AnomaliesArgs, AppsArgs, ClockSkewArgs, CompareArgs, CorrelateStateArgs, EntityArgs,
    FleetStateArgs, GraphAroundArgs, GraphEvidenceArgs, GraphExplainArgs, GraphRebuildArgs,
    GraphStatusArgs, HostStateArgs, SilentHostsArgs,
};
use super::CliMode;

impl SilentHostsArgs {
    pub(crate) fn into_request(self) -> SilentHostsRequest {
        SilentHostsRequest {
            silent_minutes: self.silent_minutes,
        }
    }
}

impl ClockSkewArgs {
    pub(crate) fn into_request(self) -> ClockSkewRequest {
        ClockSkewRequest {
            since: self.since,
            limit: self.limit,
        }
    }
}

impl AnomaliesArgs {
    pub(crate) fn into_request(self) -> AnomaliesRequest {
        AnomaliesRequest {
            recent_minutes: self.recent_minutes,
            baseline_minutes: self.baseline_minutes,
        }
    }
}

impl CompareArgs {
    pub(crate) fn into_request(self) -> Result<CompareRequest> {
        Ok(CompareRequest {
            a_from: self
                .a_from
                .ok_or_else(|| anyhow::anyhow!("--a-from is required"))?,
            a_to: self
                .a_to
                .ok_or_else(|| anyhow::anyhow!("--a-to is required"))?,
            b_from: self
                .b_from
                .ok_or_else(|| anyhow::anyhow!("--b-from is required"))?,
            b_to: self
                .b_to
                .ok_or_else(|| anyhow::anyhow!("--b-to is required"))?,
        })
    }
}

impl AppsArgs {
    pub(crate) fn into_request(self) -> ListAppsRequest {
        ListAppsRequest {
            hostname: self.hostname,
            from: self.from,
            to: self.to,
            limit: self.limit,
            offset: self.offset,
        }
    }
}

pub(crate) async fn run_silent_hosts(mode: &CliMode, args: SilentHostsArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.silent_hosts(req).await?,
        CliMode::Http(client) => http_or_cancel(client.silent_hosts(&req)).await?,
    };
    if json {
        return print_json(&response);
    }
    println!(
        "silent_minutes={} cutoff={} now={}",
        response.silent_minutes, response.cutoff, response.now
    );
    println!("{} host(s) silent:", response.hosts.len());
    for h in &response.hosts {
        println!(
            "  {:<20} last_seen={} silent_for_secs={} log_count={}",
            h.hostname, h.last_seen, h.silent_for_secs, h.log_count
        );
    }
    Ok(())
}

pub(crate) async fn run_clock_skew(mode: &CliMode, args: ClockSkewArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.clock_skew(req).await?,
        CliMode::Http(client) => http_or_cancel(client.clock_skew(&req)).await?,
    };
    if json {
        return print_json(&response);
    }
    println!("since={}", response.since);
    println!("{} host(s):", response.hosts.len());
    for h in &response.hosts {
        println!(
            "  {:<20} samples={} avg_skew={:.2}s min={:.2}s max={:.2}s",
            h.hostname, h.samples, h.avg_skew_secs, h.min_skew_secs, h.max_skew_secs
        );
    }
    Ok(())
}

pub(crate) async fn run_anomalies(mode: &CliMode, args: AnomaliesArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.anomalies(req).await?,
        CliMode::Http(client) => http_or_cancel(client.anomalies(&req)).await?,
    };
    if json {
        return print_json(&response);
    }
    println!(
        "recent={}-{} ({}m)  baseline={}-{} ({}m)",
        response.recent_from,
        response.recent_to,
        response.recent_minutes,
        response.baseline_from,
        response.baseline_to,
        response.baseline_minutes,
    );
    println!("{} host(s):", response.hosts.len());
    for h in &response.hosts {
        let ratio = h
            .ratio
            .map(|r| format!("{r:.2}"))
            .unwrap_or_else(|| "-".to_string());
        let z = h
            .z_score
            .map(|z| format!("{z:.2}"))
            .unwrap_or_else(|| "-".to_string());
        println!(
            "  {:<20} recent={:.2}/min baseline={:.2}/min ratio={} z={}",
            h.hostname, h.recent_per_min, h.baseline_per_min, ratio, z
        );
    }
    Ok(())
}

pub(crate) async fn run_compare(mode: &CliMode, args: CompareArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request()?;
    let response = match mode {
        CliMode::Local(service) => service.compare(req).await?,
        CliMode::Http(client) => http_or_cancel(client.compare(&req)).await?,
    };
    if json {
        return print_json(&response);
    }
    println!(
        "A {} → {}  total_logs={} total_errors={}",
        response.a.from, response.a.to, response.a.total_logs, response.a.total_errors
    );
    println!(
        "B {} → {}  total_logs={} total_errors={}",
        response.b.from, response.b.to, response.b.total_logs, response.b.total_errors
    );
    println!(
        "delta_total_logs={} delta_total_errors={}",
        response.delta_total_logs, response.delta_total_errors
    );
    Ok(())
}

pub(crate) async fn run_apps(mode: &CliMode, args: AppsArgs) -> Result<()> {
    let json = args.json;
    let req = args.into_request();
    let response = match mode {
        CliMode::Local(service) => service.list_apps(req).await?,
        CliMode::Http(client) => http_or_cancel(client.list_apps(&req)).await?,
    };
    if json {
        return print_json(&response);
    }
    println!("{} app(s) (total {}):", response.apps.len(), response.total);
    for a in &response.apps {
        println!(
            "  {:<24} logs={} hosts={} first_seen={} last_seen={}",
            a.app_name, a.log_count, a.host_count, a.first_seen, a.last_seen
        );
    }
    Ok(())
}

// ─── Heartbeat fleet state (cxih.4) ─────────────────────────────────────────

impl HostStateArgs {
    pub(crate) fn into_request(self) -> HostStateRequest {
        HostStateRequest {
            host_id: self.host_id,
            hostname: self.hostname,
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
        CliMode::Local(service) => service.host_state(req).await?,
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
        CliMode::Local(service) => service.fleet_state(req).await?,
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
