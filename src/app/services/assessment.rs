use super::*;

impl CortexService {
    pub async fn run_gemini_assess(&self, req: AiAssessRequest) -> ServiceResult<AiAssessResponse> {
        self.run_gemini_assess_with_delta(req, |_| Ok(())).await
    }

    pub async fn run_gemini_assess_with_delta<F>(
        &self,
        req: AiAssessRequest,
        on_delta: F,
    ) -> ServiceResult<AiAssessResponse>
    where
        F: FnMut(&str) -> anyhow::Result<()> + Send,
    {
        let incident_id = req.incident_id.clone();
        let gemini_config = GeminiAssessConfig::from_env(req.model);
        let invest_req = AiInvestigateRequest {
            incident_id: Some(incident_id.clone()),
            project: req.project,
            tool: req.tool,
            since: req.since,
            until: req.until,
            limit: Some(req.limit.unwrap_or(200).max(200)),
            window_minutes: req.window_minutes,
            correlation_window_minutes: req.correlation_window_minutes,
            terms: req.terms,
        };
        let invest_resp = self.investigate_ai_incidents(invest_req).await?;

        let matching: Vec<_> = invest_resp
            .evidence
            .iter()
            .filter(|e| e.incident.incident_id == incident_id)
            .collect();

        if matching.is_empty() {
            return Err(ServiceError::InvalidInput(format!(
                "no incident found with id '{}'; run `cortex ai incidents` to list available ids",
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

        let assessment = run_gemini_assessment(&prompt, &gemini_config, on_delta)
            .await
            .map_err(ServiceError::Internal)?;

        Ok(AiAssessResponse {
            incident_id,
            assessment,
            prompt_preview,
            evidence_summary,
        })
    }
}
