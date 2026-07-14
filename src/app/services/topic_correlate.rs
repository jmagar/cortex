//! `topic_correlate` — graph-anchored universal correlation.
//!
//! Resolve a free-text topic to graph entities, expand the graph, and return a
//! unified timeline of logs from every related source annotated by source kind
//! and discovery lane. Graph-first: traverse → entity set → log join.

use super::*;

/// Default log fan-out cap when the caller omits `--limit`.
const TOPIC_DEFAULT_LIMIT: u32 = 200;
/// Default graph traversal depth.
const TOPIC_DEFAULT_DEPTH: u8 = 2;

impl CortexService {
    pub async fn topic_correlate(
        &self,
        req: TopicCorrelateRequest,
    ) -> ServiceResult<TopicCorrelateResponse> {
        let since = parse_optional_timestamp(req.since.as_deref(), "since")?;
        let until = parse_optional_timestamp(req.until.as_deref(), "until")?;

        // Hard break: legacy nested service identities (`tootie:plex`,
        // `tootie:plex:plex`, `plex/plex/plex`) are rejected before any
        // graph lookup runs.
        if let Some(rejected) = super::graph_support::reject_legacy_service_identity(&req.topic) {
            return Err(rejected);
        }

        // Split the topic into lowercased, de-duplicated terms (OR semantics).
        let mut terms: Vec<String> = req
            .topic
            .split_whitespace()
            .map(|t| t.to_ascii_lowercase())
            .collect();
        terms.sort();
        terms.dedup();

        let depth = req.depth.unwrap_or(TOPIC_DEFAULT_DEPTH);
        let limit = req.limit.unwrap_or(TOPIC_DEFAULT_LIMIT).clamp(1, 1000) as usize;

        let source_kinds: Option<Vec<crate::enrich::parser::SourceKind>> = match req
            .source_kinds
            .as_ref()
        {
            Some(kinds) => {
                let mut parsed = Vec::with_capacity(kinds.len());
                let mut invalid = Vec::new();
                for kind in kinds {
                    match crate::enrich::parser::SourceKind::from_wire(kind) {
                        Some(kind) => parsed.push(kind),
                        None => invalid.push(kind.clone()),
                    }
                }
                if !invalid.is_empty() {
                    return Err(ServiceError::InvalidInput(format!(
                        "invalid source_kinds: {}. Expected one or more kebab-case source kinds",
                        invalid.join(", ")
                    )));
                }
                (!parsed.is_empty()).then_some(parsed)
            }
            None => None,
        };

        if terms.is_empty() {
            return Ok(empty_topic_response(req.topic));
        }

        let topic = req.topic.clone();
        let (inputs, summaries) = self
            .run_db(
                "topic_correlate",
                move |pool| -> anyhow::Result<(
                    db::TopicGraphInputs,
                    Vec<db::HeartbeatWindowSummary>,
                )> {
                    let inputs = db::topic_correlate_inputs(
                        pool,
                        &terms,
                        depth,
                        since.as_deref(),
                        until.as_deref(),
                        source_kinds.as_deref(),
                        limit,
                    )?;

                    // Heartbeat window: explicit bounds, else the log span.
                    let window = heartbeat_window(&inputs.logs, since.as_deref(), until.as_deref());
                    let summaries = match (&window, inputs.discovered_hosts.is_empty()) {
                        (Some((from, to)), false) => {
                            db::heartbeat_window_summaries(pool, from, to, None)?
                        }
                        _ => Vec::new(),
                    };
                    Ok((inputs, summaries))
                },
            )
            .await?;

        Ok(build_topic_response(topic, inputs, summaries, limit))
    }
}

fn empty_topic_response(topic: String) -> TopicCorrelateResponse {
    TopicCorrelateResponse {
        topic,
        resolved_entities: Vec::new(),
        graph_expansion: Vec::new(),
        discovered_hosts: Vec::new(),
        timeline: Vec::new(),
        heartbeat_summaries: Vec::new(),
        truncated: false,
    }
}

/// Compute the heartbeat window: explicit `[since, until]` when both given,
/// otherwise the min/max timestamp across the correlated logs.
fn heartbeat_window(
    logs: &[db::GraphRelatedLogEntry],
    since: Option<&str>,
    until: Option<&str>,
) -> Option<(String, String)> {
    if let (Some(from), Some(to)) = (since, until) {
        return Some((from.to_string(), to.to_string()));
    }
    let min = logs.iter().map(|l| l.entry.timestamp.as_str()).min()?;
    let max = logs.iter().map(|l| l.entry.timestamp.as_str()).max()?;
    Some((
        since.unwrap_or(min).to_string(),
        until.unwrap_or(max).to_string(),
    ))
}

fn build_topic_response(
    topic: String,
    inputs: db::TopicGraphInputs,
    summaries: Vec<db::HeartbeatWindowSummary>,
    limit: usize,
) -> TopicCorrelateResponse {
    let truncated = inputs.logs.len() >= limit;

    let resolved_entities = inputs
        .resolved
        .into_iter()
        .map(|r| ResolvedTopicEntity {
            entity_type: r.entity_type,
            key: r.canonical_key,
            match_kind: r.match_kind.to_string(),
            resolver_status: Some(r.resolver_status.to_string()),
        })
        .collect();

    let graph_expansion = inputs
        .expansion
        .into_iter()
        .map(|(entity_type, key)| TopicExpansionEntity { entity_type, key })
        .collect();

    let timeline = inputs
        .logs
        .into_iter()
        .map(|related| {
            let entry = related.entry;
            let source_kind = row_source_kind(&entry);
            let entity_path = if entry.source_ip.starts_with("agent-command://") {
                "agent_command".to_string()
            } else if source_kind.as_deref() == Some("shell-history") {
                "shell_history".to_string()
            } else {
                format!("graph:host:{}", entry.hostname)
            };
            TopicTimelineEntry {
                timestamp: entry.timestamp,
                source_kind,
                entity_path,
                hostname: entry.hostname,
                app_name: entry.app_name,
                message: entry.message,
                session_id: entry.ai_session_id,
                inclusion_reason: Some(related.inclusion_reason),
                resolver_status: Some(related.resolver_status),
                fallback_kind: related.fallback_kind,
            }
        })
        .collect();

    let discovered: std::collections::HashSet<&str> =
        inputs.discovered_hosts.iter().map(String::as_str).collect();
    let heartbeat_summaries = summaries
        .into_iter()
        .filter(|s| discovered.contains(s.hostname.as_str()))
        .collect();

    TopicCorrelateResponse {
        topic,
        resolved_entities,
        graph_expansion,
        discovered_hosts: inputs.discovered_hosts,
        timeline,
        heartbeat_summaries,
        truncated,
    }
}

#[cfg(test)]
#[path = "topic_correlate_tests.rs"]
mod topic_correlate_tests;
