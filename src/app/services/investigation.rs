use std::time::Instant;

use super::*;

impl CortexService {
    pub async fn investigation_ask(
        &self,
        req: AskInvestigationRequest,
    ) -> ServiceResult<InvestigationEnvelope<AskInvestigationResponse>> {
        let started = Instant::now();
        let budget = InvestigationBudget::default();
        let prompt = safe_passive_text(req.prompt.trim(), 1_000);
        if prompt.is_empty() {
            return Err(ServiceError::InvalidInput(
                "prompt must not be empty".to_string(),
            ));
        }

        let log_limit = req.limit.unwrap_or(12).clamp(1, budget.max_log_rows);
        let host_hint = req
            .host
            .as_deref()
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .map(|host| safe_passive_text(host, 120));

        let logs = self
            .investigation_log_context(
                &prompt,
                host_hint.clone(),
                req.since.clone(),
                req.until.clone(),
                log_limit,
            )
            .await?;
        let target_host = host_hint.or_else(|| logs.first().map(|log| log.hostname.clone()));

        let mut graph_calls = 0;
        let explain = if let Some(host) = target_host.as_ref() {
            graph_calls += 1;
            Some(
                self.graph_explain(GraphExplainRequest {
                    entity_type: Some("host".to_string()),
                    key: Some(host.clone()),
                    depth: Some(2),
                    beam_width: Some(20),
                    max_chains: Some(budget.max_candidate_explanations),
                    evidence_sample_limit: Some(2),
                    payload_budget: Some(budget.max_payload_bytes),
                    ..Default::default()
                })
                .await?,
            )
        } else {
            None
        };

        let metadata = investigation_metadata(
            &budget,
            started,
            graph_calls,
            logs.len() as u32,
            explain.as_ref(),
        );
        let result = ask_response(prompt, logs, explain);
        Ok(InvestigationEnvelope { metadata, result })
    }

    async fn investigation_log_context(
        &self,
        prompt: &str,
        host: Option<String>,
        since: Option<String>,
        until: Option<String>,
        limit: u32,
    ) -> ServiceResult<Vec<LogEntry>> {
        let query = fts_query_from_prompt(prompt);
        let search = self
            .search_logs(SearchLogsRequest {
                query,
                host: host.clone(),
                since,
                until,
                limit: Some(limit),
                ..Default::default()
            })
            .await;

        match search {
            Ok(response) => Ok(response.logs),
            Err(error) => {
                tracing::warn!(%error, "investigation prompt search failed; falling back to tail");
                Ok(self
                    .tail_logs(TailLogsRequest {
                        host,
                        n: Some(limit),
                        ..Default::default()
                    })
                    .await?
                    .logs)
            }
        }
    }
}

fn fts_query_from_prompt(prompt: &str) -> Option<String> {
    let terms = prompt
        .split_whitespace()
        .map(|term| term.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-'))
        .filter(|term| term.chars().count() >= 3)
        .take(6)
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

fn ask_response(
    prompt: String,
    logs: Vec<LogEntry>,
    explain: Option<GraphExplainResponse>,
) -> AskInvestigationResponse {
    let safe_logs = logs
        .iter()
        .map(|log| app_log_summary(log, 500))
        .collect::<Vec<_>>();

    let Some(explain) = explain else {
        return AskInvestigationResponse {
            prompt,
            resolved_entity: None,
            candidates: Vec::new(),
            claims: vec![InvestigationClaim {
                claim_type: InvestigationClaimType::OpenQuestion,
                title: "No graph entity resolved".to_string(),
                summary: "Cortex found log context, but no host or graph entity was resolved for a defensible explanation.".to_string(),
                confidence: "open_question".to_string(),
                relationship_ids: Vec::new(),
                evidence_ids: Vec::new(),
            }],
            open_questions: vec!["Which host or service should Cortex inspect?".to_string()],
            next_queries: Vec::new(),
            graph: AppGraphResponse {
                focus: None,
                entities: Vec::new(),
                relationships: Vec::new(),
                evidence: Vec::new(),
            },
            logs: safe_logs,
        };
    };

    let graph = app_graph_from_explain_response(&explain);
    let mut claims = Vec::new();
    if let Some(narrative) = explain.narrative.as_ref() {
        claims.push(InvestigationClaim {
            claim_type: InvestigationClaimType::SupportedCorrelation,
            title: safe_passive_text(&narrative.title, 180),
            summary: safe_passive_text(&narrative.summary, 700),
            confidence: narrative.confidence.clone(),
            relationship_ids: narrative.relationship_ids.clone(),
            evidence_ids: narrative.evidence_ids.clone(),
        });
    }
    claims.extend(
        explain
            .chains
            .iter()
            .take(4)
            .map(|chain| InvestigationClaim {
                claim_type: InvestigationClaimType::SupportedCorrelation,
                title: format!("Evidence chain {}", chain.chain_id),
                summary: safe_passive_text(&chain.summary, 700),
                confidence: chain.confidence.clone(),
                relationship_ids: chain.relationship_ids.clone(),
                evidence_ids: chain.evidence_ids.clone(),
            }),
    );
    if claims.is_empty() {
        claims.push(InvestigationClaim {
            claim_type: InvestigationClaimType::OpenQuestion,
            title: "Explanation needs more evidence".to_string(),
            summary: "Cortex resolved the graph focus, but did not find enough relationship evidence to present a supported explanation.".to_string(),
            confidence: "open_question".to_string(),
            relationship_ids: Vec::new(),
            evidence_ids: Vec::new(),
        });
    }

    let next_queries = explain
        .next_queries
        .iter()
        .map(|next| safe_passive_text(&next.label, 160))
        .collect::<Vec<_>>();
    AskInvestigationResponse {
        prompt,
        resolved_entity: explain.resolved_entity.as_ref().map(app_entity_summary),
        candidates: explain
            .candidates
            .iter()
            .map(|candidate| app_entity_summary(&candidate.entity))
            .collect(),
        claims,
        open_questions: explain
            .open_questions
            .iter()
            .map(|question| safe_passive_text(question, 300))
            .collect(),
        next_queries,
        graph,
        logs: safe_logs,
    }
}

fn investigation_metadata(
    budget: &InvestigationBudget,
    started: Instant,
    graph_calls: u32,
    log_rows: u32,
    explain: Option<&GraphExplainResponse>,
) -> InvestigationMetadata {
    let graph_metadata = explain.map(|explain| &explain.metadata);
    let truncated = graph_metadata.is_some_and(|metadata| metadata.truncated);
    let truncation_reasons = graph_metadata
        .and_then(|metadata| metadata.truncated_reason.clone())
        .into_iter()
        .collect::<Vec<_>>();
    let degraded_reasons = graph_metadata
        .filter(|metadata| metadata.is_degraded)
        .and_then(|metadata| metadata.last_error.clone())
        .into_iter()
        .collect::<Vec<_>>();
    InvestigationMetadata {
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: INVESTIGATION_UI_VERSION.to_string(),
        graph_projection_status: graph_metadata.map(|metadata| metadata.projection_status.clone()),
        source_watermark: graph_metadata.map(|metadata| metadata.source_watermark.clone()),
        degraded_reasons,
        truncated,
        truncation_reasons,
        partial: truncated,
        partial_reasons: if truncated {
            vec!["graph_response_truncated".to_string()]
        } else {
            Vec::new()
        },
        auth_state: "bearer".to_string(),
        budget: budget.clone(),
        budget_used: InvestigationBudgetUsed {
            graph_calls,
            log_rows,
            evidence_rows: explain.map_or(0, |explain| explain.evidence.len() as u32),
            candidate_explanations: explain.map_or(0, |explain| explain.chains.len() as u32),
            wall_time_ms: started.elapsed().as_millis().min(u32::MAX as u128) as u32,
            payload_bytes: 0,
        },
        payload_limit_bytes: budget.max_payload_bytes,
        version_skew: None,
    }
}
