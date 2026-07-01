use super::*;
use crate::app::llm_runner::{
    LlmCallerSurface, LlmDryRunOutcome, LlmEvidenceCounts, LlmInvocationSpec,
};

impl CortexService {
    pub async fn run_gemini_assess(&self, req: AiAssessRequest) -> ServiceResult<AiAssessResponse> {
        self.run_gemini_assess_with_delta(req, |_| Ok(())).await
    }

    /// Preview the prompt/evidence bundle that `run_gemini_assess` would
    /// send to Gemini, WITHOUT invoking the LLM — routes through
    /// `LlmRunner::dry_run` (GH issue #94's acceptance criterion for a
    /// dry-run/preview mode). Still writes an audit row (status
    /// "dry_run"), same as `LlmRunner::dry_run` does for every other
    /// caller, but never spawns the Gemini subprocess.
    pub async fn dry_run_gemini_assess(
        &self,
        req: AiAssessRequest,
    ) -> ServiceResult<LlmDryRunOutcome> {
        let incident_id = req.incident_id.clone();
        let gemini_config =
            GeminiAssessConfig::from_env(req.model.clone(), self.llm().timeout_secs());
        let invest_req = AiInvestigateRequest {
            incident_id: Some(incident_id.clone()),
            project: req.project.clone(),
            tool: req.tool.clone(),
            since: req.since.clone(),
            until: req.until.clone(),
            limit: Some(req.limit.unwrap_or(200).max(200)),
            window_minutes: req.window_minutes,
            correlation_window_minutes: req.correlation_window_minutes,
            terms: req.terms.clone(),
        };
        let invest_resp = self.investigate_ai_incidents(invest_req).await?;

        let matching: Vec<_> = invest_resp
            .evidence
            .iter()
            .filter(|e| e.incident.incident_id == incident_id)
            .collect();

        if matching.is_empty() {
            return Err(ServiceError::InvalidInput(format!(
                "no incident found with id '{}'; run `cortex sessions incidents` to list available ids",
                incident_id
            )));
        }

        let evidence_json = serde_json::to_string_pretty(&matching)
            .map_err(|e| ServiceError::Internal(anyhow::anyhow!("json serialize failed: {e}")))?;
        let prompt = build_assessment_prompt(&evidence_json);
        let evidence_counts = LlmEvidenceCounts {
            total_incidents: invest_resp.total_incidents,
            evidence_bundle_count: matching.len(),
            total_anchors: matching.iter().map(|e| e.anchors.len()).sum(),
            truncated: invest_resp.truncated,
        };

        let spec = LlmInvocationSpec {
            caller_surface: LlmCallerSurface::Cli,
            action: "ai_assess".to_string(),
            incident_id: Some(incident_id.clone()),
            ai_tool: req.tool.clone(),
            ai_project: req.project.clone(),
            ai_session_id: None,
            evidence_counts,
            prompt,
            provider: "gemini-cli".to_string(),
            model: gemini_config.model.clone(),
            program: gemini_config.program.clone(),
            extra_metadata: serde_json::json!({}),
        };

        self.llm()
            .dry_run(&spec)
            .await
            .map_err(|err| ServiceError::Internal(anyhow::anyhow!(err)))
    }

    pub async fn run_gemini_assess_with_delta<F>(
        &self,
        req: AiAssessRequest,
        mut on_delta: F,
    ) -> ServiceResult<AiAssessResponse>
    where
        F: FnMut(&str) -> anyhow::Result<()> + Send,
    {
        let incident_id = req.incident_id.clone();
        // Eng review fix (Fix 1): pass LlmRunner's own resolved timeout
        // through instead of letting GeminiAssessConfig::from_env
        // independently re-read CORTEX_LLM_COMPLETION_TIMEOUT_SECS.
        let gemini_config =
            GeminiAssessConfig::from_env(req.model.clone(), self.llm().timeout_secs());
        let invest_req = AiInvestigateRequest {
            incident_id: Some(incident_id.clone()),
            project: req.project.clone(),
            tool: req.tool.clone(),
            since: req.since.clone(),
            until: req.until.clone(),
            limit: Some(req.limit.unwrap_or(200).max(200)),
            window_minutes: req.window_minutes,
            correlation_window_minutes: req.correlation_window_minutes,
            terms: req.terms.clone(),
        };
        let invest_resp = self.investigate_ai_incidents(invest_req).await?;

        let matching: Vec<_> = invest_resp
            .evidence
            .iter()
            .filter(|e| e.incident.incident_id == incident_id)
            .collect();

        if matching.is_empty() {
            return Err(ServiceError::InvalidInput(format!(
                "no incident found with id '{}'; run `cortex sessions incidents` to list available ids",
                incident_id
            )));
        }

        let evidence_json = serde_json::to_string_pretty(&matching)
            .map_err(|e| ServiceError::Internal(anyhow::anyhow!("json serialize failed: {e}")))?;
        let prompt = build_assessment_prompt(&evidence_json);
        let prompt_preview = prompt.chars().take(500).collect::<String>();
        let evidence_summary = AiAssessEvidenceSummary {
            total_incidents: invest_resp.total_incidents,
            evidence_bundle_count: matching.len(),
            total_anchors: matching.iter().map(|e| e.anchors.len()).sum(),
        };

        // `on_delta` is `FnMut` and borrows the caller's stack, so it cannot
        // cross into the `'static` `run_fn` closure `LlmRunner::run`
        // requires. Stream deltas through a channel instead: the run_fn
        // task forwards each parsed delta line, and this function drains
        // the channel concurrently with awaiting the run.
        let (delta_tx, mut delta_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let gemini_config_owned = gemini_config.clone();
        let run_fut = self.llm().run(
            LlmInvocationSpec {
                caller_surface: LlmCallerSurface::Cli,
                action: "ai_assess".to_string(),
                incident_id: Some(incident_id.clone()),
                ai_tool: req.tool.clone(),
                ai_project: req.project.clone(),
                ai_session_id: None,
                evidence_counts: LlmEvidenceCounts {
                    total_incidents: evidence_summary.total_incidents,
                    evidence_bundle_count: evidence_summary.evidence_bundle_count,
                    total_anchors: evidence_summary.total_anchors,
                    truncated: invest_resp.truncated,
                },
                prompt: prompt.clone(),
                provider: "gemini-cli".to_string(),
                model: gemini_config_owned.model.clone(),
                program: gemini_config_owned.program.clone(),
                extra_metadata: serde_json::json!({}),
            },
            move |prompt| async move {
                run_gemini_assessment(&prompt, &gemini_config_owned, move |delta: &str| {
                    let _ = delta_tx.send(delta.to_string());
                    Ok(())
                })
                .await
            },
        );
        tokio::pin!(run_fut);

        let assessment = loop {
            tokio::select! {
                biased;
                Some(delta) = delta_rx.recv() => {
                    on_delta(&delta).map_err(ServiceError::Internal)?;
                }
                result = &mut run_fut => {
                    // Drain any remaining buffered deltas before returning.
                    while let Ok(delta) = delta_rx.try_recv() {
                        on_delta(&delta).map_err(ServiceError::Internal)?;
                    }
                    break result;
                }
            }
        }
        .map_err(|err| ServiceError::Internal(anyhow::anyhow!(err)))?
        .output;

        Ok(AiAssessResponse {
            incident_id,
            assessment,
            prompt_preview,
            evidence_summary,
        })
    }
}
