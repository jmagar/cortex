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

        // Parse requested source kinds, dropping unrecognised values. An empty
        // result (none valid) means "no filter" rather than "match nothing".
        let source_kinds: Option<Vec<crate::enrich::parser::SourceKind>> =
            req.source_kinds.as_ref().and_then(|kinds| {
                let parsed: Vec<_> = kinds
                    .iter()
                    .filter_map(|k| crate::enrich::parser::SourceKind::from_wire(k))
                    .collect();
                (!parsed.is_empty()).then_some(parsed)
            });

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
    logs: &[db::LogEntry],
    since: Option<&str>,
    until: Option<&str>,
) -> Option<(String, String)> {
    if let (Some(from), Some(to)) = (since, until) {
        return Some((from.to_string(), to.to_string()));
    }
    let min = logs.iter().map(|l| l.timestamp.as_str()).min()?;
    let max = logs.iter().map(|l| l.timestamp.as_str()).max()?;
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
        .map(|entry| {
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
