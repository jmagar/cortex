use super::filters::{filter_request_to_params, search_request_to_params};
use super::*;

impl CortexService {
    pub async fn health_check(&self) -> ServiceResult<()> {
        self.run_db("health_check", |pool| {
            let conn = pool.get()?;
            conn.query_row("SELECT 1", [], |_| Ok(()))?;
            Ok(())
        })
        .await
    }

    pub async fn search_logs(&self, req: SearchLogsRequest) -> ServiceResult<SearchLogsResponse> {
        let query = req.query.clone();
        let params = search_request_to_params(req)?;
        let params = SearchParams { query, ..params };
        let logs = self
            .run_db("search_logs", move |pool| db::search_logs(pool, &params))
            .await?;
        let logs: Vec<LogEntry> = logs.into_iter().map(Into::into).collect();
        Ok(SearchLogsResponse {
            count: logs.len(),
            logs,
        })
    }

    pub async fn host_state(
        &self,
        req: models::HostStateRequest,
    ) -> ServiceResult<models::HostStateResponse> {
        let lookup = match (req.host_id, req.hostname) {
            (Some(host_id), _) if !host_id.trim().is_empty() => {
                db::HeartbeatHostLookup::HostId(host_id)
            }
            (_, Some(hostname)) if !hostname.trim().is_empty() => {
                db::HeartbeatHostLookup::Hostname(hostname)
            }
            _ => {
                return Err(ServiceError::InvalidInput(
                    "host_state requires host_id or hostname".into(),
                ));
            }
        };
        let limit = req.limit.unwrap_or(1).clamp(1, 100) as usize;
        let since = parse_optional_timestamp(req.since.as_deref(), "since")?;
        self.run_db("host_state", move |pool| {
            db::heartbeat_host_state(pool, lookup, since.as_deref(), limit).map_err(|error| {
                match error.to_string().as_str() {
                    "not_found" => anyhow::anyhow!("not_found"),
                    "ambiguous_host" => anyhow::anyhow!("ambiguous_host"),
                    _ => error,
                }
            })
        })
        .await
        .map_err(|error| match error {
            ServiceError::Internal(error) if error.to_string() == "not_found" => {
                ServiceError::NotFound("host_state host not found".into())
            }
            ServiceError::Internal(error) if error.to_string() == "ambiguous_host" => {
                ServiceError::InvalidInput("ambiguous_host".into())
            }
            other => other,
        })
    }

    pub async fn fleet_state(&self, req: FleetStateRequest) -> ServiceResult<FleetStateResponse> {
        let include_ok = req.include_ok.unwrap_or(true);
        let sort = req.sort.clone().unwrap_or_else(|| "pressure".into());

        let entries = self
            .run_db("fleet_state.latest", db::heartbeat_latest_all)
            .await?;

        let mut rows: Vec<FleetStateHostRow> = Vec::with_capacity(entries.len());
        for entry in &entries {
            let hb_id = entry.heartbeat_id;
            let metrics = self
                .run_db("fleet_state.metrics", move |pool| {
                    db::heartbeat_metric_snapshot(pool, hb_id)
                })
                .await?;
            let flags = heartbeat_flags::from_latest_and_metrics(entry, &metrics);
            let pressure = heartbeat_flags::pressure_names(&flags);
            let status = heartbeat_flags::host_status_label(&flags);
            if !include_ok && status == "ok" {
                continue;
            }
            rows.push(FleetStateHostRow {
                host_id: entry.host_id.clone(),
                hostname: entry.hostname.clone(),
                last_heartbeat_at: entry.sampled_at.clone(),
                status: status.to_owned(),
                pressure,
                partial: flags.collector_partial,
                clock_skew: flags.clock_skew,
            });
        }

        // Sort
        match sort.as_str() {
            "freshness" => rows.sort_by(|a, b| b.last_heartbeat_at.cmp(&a.last_heartbeat_at)),
            "hostname" => rows.sort_by(|a, b| a.hostname.cmp(&b.hostname)),
            _ => {
                // pressure sort: late > partial > pressure > ok, then hostname
                let rank = |status: &str| match status {
                    "late" => 0,
                    "partial" => 1,
                    "pressure" => 2,
                    _ => 3,
                };
                rows.sort_by(|a, b| {
                    rank(a.status.as_str())
                        .cmp(&rank(b.status.as_str()))
                        .then_with(|| a.hostname.cmp(&b.hostname))
                });
            }
        }

        let total = rows.len();
        let mut summary = FleetStateSummary {
            total,
            ..Default::default()
        };
        for row in &rows {
            match row.status.as_str() {
                "ok" => summary.ok += 1,
                "late" => summary.late += 1,
                "partial" => summary.partial += 1,
                "pressure" => summary.pressure += 1,
                _ => {}
            }
        }

        Ok(FleetStateResponse {
            hosts: rows,
            summary,
        })
    }

    pub async fn correlate_state(
        &self,
        req: CorrelateStateRequest,
    ) -> ServiceResult<CorrelateStateResponse> {
        let reference_time = req.reference_time.trim().to_owned();
        if reference_time.is_empty() {
            return Err(ServiceError::InvalidInput(
                "correlate_state requires reference_time".into(),
            ));
        }
        let ref_dt = parse_required_timestamp(&reference_time, "reference_time")
            .map_err(|_| ServiceError::InvalidInput("invalid reference_time format".into()))?;

        let window_minutes = req.window_minutes.unwrap_or(10).clamp(1, 120) as i64;
        let limit = req.limit.unwrap_or(100).clamp(1, 500) as usize;
        let severity_min = req.severity_min.clone().unwrap_or_else(|| "info".into());

        let from_dt = ref_dt - TimeDelta::minutes(window_minutes);
        let to_dt = ref_dt + TimeDelta::minutes(window_minutes);
        let from = rfc3339_z(from_dt);
        let to = rfc3339_z(to_dt);

        // Resolve optional host filter
        let host_id: Option<String> = if let Some(host) = req.host.as_deref() {
            let h = host.to_owned();
            let resolved = self
                .run_db("correlate_state.resolve_host", move |pool| {
                    let conn = pool.get()?;
                    // Try host_id first
                    let exists: bool = conn
                        .query_row(
                            "SELECT COUNT(*) FROM host_heartbeats_latest WHERE host_id = ?1",
                            [&h],
                            |row| row.get::<_, i64>(0),
                        )
                        .map(|c| c > 0)?;
                    if exists {
                        return Ok(Some(h));
                    }
                    // Hostname fallback (unique only)
                    let mut stmt = conn.prepare(
                        "SELECT host_id FROM host_heartbeats_latest
                         WHERE hostname = ?1",
                    )?;
                    let ids: Vec<String> = stmt
                        .query_map([&h], |row| row.get(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    match ids.as_slice() {
                        [] => Err(anyhow::anyhow!("not_found")),
                        [id] => Ok(Some(id.clone())),
                        _ => Err(anyhow::anyhow!("ambiguous_host")),
                    }
                })
                .await
                .map_err(|e| match e {
                    ServiceError::Internal(ref inner) if inner.to_string() == "not_found" => {
                        ServiceError::NotFound("correlate_state host not found".into())
                    }
                    ServiceError::Internal(ref inner) if inner.to_string() == "ambiguous_host" => {
                        ServiceError::InvalidInput("ambiguous_host".into())
                    }
                    other => other,
                })?;
            resolved
        } else {
            None
        };

        let from2 = from.clone();
        let to2 = to.clone();
        let hid = host_id.clone();
        let heartbeat_summaries = self
            .run_db("correlate_state.heartbeats", move |pool| {
                db::heartbeat_window_summaries(pool, &from2, &to2, hid.as_deref())
            })
            .await?;

        // Fetch logs for each host in the window
        let mut host_entries: Vec<CorrelateStateHostEntry> = Vec::new();
        let mut global_truncated = false;
        for summary in heartbeat_summaries {
            let host_id_filter = summary.host_id.clone();
            let hostname_filter = summary.hostname.clone();
            let from3 = from.clone();
            let to3 = to.clone();
            let sev_levels = correlate::severity_at_or_above(&severity_min)?;
            let fetch_limit = limit + 1;
            let logs = self
                .run_db("correlate_state.logs", move |pool| {
                    db::search_logs(
                        pool,
                        &db::SearchParams {
                            hostname: Some(hostname_filter.clone()),
                            from: Some(from3),
                            to: Some(to3),
                            severity_in: Some(sev_levels),
                            limit: Some(fetch_limit as u32),
                            // correlate_state correlates non-AI logs with heartbeat
                            // state; AI transcript rows must never appear here.
                            exclude_ai: true,
                            ..Default::default()
                        },
                    )
                })
                .await?;
            let truncated = logs.len() > limit;
            if truncated {
                global_truncated = true;
            }
            let log_entries: Vec<LogEntry> = logs
                .into_iter()
                .take(limit)
                .filter(|log| {
                    // Filter to the specific host_id when known
                    log.hostname == summary.hostname || log.hostname == host_id_filter
                })
                .map(LogEntry::from)
                .collect();

            host_entries.push(CorrelateStateHostEntry {
                host_id: summary.host_id.clone(),
                hostname: summary.hostname.clone(),
                heartbeat_summary: summary,
                logs: log_entries,
            });
        }

        Ok(CorrelateStateResponse {
            window: CorrelateStateWindow { from, to },
            hosts: host_entries,
            truncated: global_truncated,
        })
    }

    pub async fn filter_logs(&self, req: FilterLogsRequest) -> ServiceResult<SearchLogsResponse> {
        let params = filter_request_to_params(req)?;
        let logs = self
            .run_db("filter_logs", move |pool| db::search_logs(pool, &params))
            .await?;
        let logs: Vec<LogEntry> = logs.into_iter().map(Into::into).collect();
        Ok(SearchLogsResponse {
            count: logs.len(),
            logs,
        })
    }

    pub async fn tail_logs(&self, req: TailLogsRequest) -> ServiceResult<SearchLogsResponse> {
        let severity_in = match req.severity_min.as_deref() {
            Some(min) => Some(severity_at_or_above(min)?),
            None => None,
        };
        let logs = self
            .run_db("tail_logs", move |pool| {
                db::tail_logs(
                    pool,
                    req.hostname.as_deref(),
                    req.source_ip.as_deref(),
                    req.app_name.as_deref(),
                    severity_in.as_deref(),
                    req.n.unwrap_or(50),
                )
            })
            .await?;
        let logs: Vec<LogEntry> = logs.into_iter().map(Into::into).collect();
        Ok(SearchLogsResponse {
            count: logs.len(),
            logs,
        })
    }

    pub async fn get_errors(&self, req: GetErrorsRequest) -> ServiceResult<GetErrorsResponse> {
        let from = parse_optional_timestamp(req.from.as_deref(), "from")?;
        let to = parse_optional_timestamp(req.to.as_deref(), "to")?;
        let group_by_app = match req.group_by.as_deref() {
            None => false,
            Some("app_name") | Some("app") => true,
            Some(other) => {
                return Err(ServiceError::InvalidInput(format!(
                    "Invalid group_by '{other}'. Supported: app_name"
                )));
            }
        };
        let rows = self
            .run_db("get_errors", move |pool| {
                db::get_error_summary(
                    pool,
                    from.as_deref(),
                    to.as_deref(),
                    group_by_app,
                    req.limit.map(|limit| limit.clamp(1, 100)),
                )
            })
            .await?;
        Ok(GetErrorsResponse {
            summary: rows.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn list_hosts(&self) -> ServiceResult<ListHostsResponse> {
        let rows = self.run_db("list_hosts", db::list_hosts).await?;
        Ok(ListHostsResponse {
            hosts: rows.into_iter().map(Into::into).collect(),
        })
    }
}
