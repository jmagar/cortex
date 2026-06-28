//! CLI dispatch for log-analytics commands (hosts silent, clock-skew,
//! anomalies, compare, apps). Split out of `dispatch_surface_gap` to keep each
//! module under the production module-size budget; the heartbeat/graph state
//! commands remain in `dispatch_surface_gap`.

use super::dispatch::http_or_cancel;
use super::output_common::print_json;

use anyhow::Result;
use cortex::app::{
    AnomaliesRequest, ClockSkewRequest, CompareRequest, ListAppsRequest, SilentHostsRequest,
    StateRequest, StateResponse,
};

use super::CliMode;
use super::args::{AnomaliesArgs, AppsArgs, ClockSkewArgs, CompareArgs, SilentHostsArgs};

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
            host: self.host,
            since: self.since,
            until: self.until,
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
        CliMode::Local(service) => match service.state(StateRequest::ClockSkew(req)).await? {
            StateResponse::ClockSkew(response) => response,
            StateResponse::Host(_) | StateResponse::Fleet(_) => {
                anyhow::bail!("internal: state clock-skew returned wrong response")
            }
        },
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

#[cfg(test)]
#[path = "dispatch_surface_analytics_tests.rs"]
mod tests;
