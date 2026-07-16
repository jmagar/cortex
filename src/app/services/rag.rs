use super::filters::validate_optional_severity;
use super::*;

impl CortexService {
    /// List recent notification firings.
    pub async fn notifications_recent(
        &self,
        limit: i64,
        rule_id: Option<String>,
        since: Option<String>,
    ) -> ServiceResult<Vec<crate::db::notifications::FiringRow>> {
        self.notifications_recent_checked(NotificationsRecentRequest {
            limit: Some(limit),
            rule_id,
            since,
        })
        .await
    }

    pub async fn notifications_recent_checked(
        &self,
        req: NotificationsRecentRequest,
    ) -> ServiceResult<Vec<crate::db::notifications::FiringRow>> {
        let limit = req.effective_limit();
        self.run_db("notifications_recent", move |pool| {
            let conn = pool.get()?;
            crate::db::notifications::firings_recent(
                &conn,
                limit,
                req.rule_id.as_deref(),
                req.since.as_deref(),
            )
            .map_err(anyhow::Error::from)
        })
        .await
    }

    /// List recent `llm_invocations` audit records (concurrency/rate-limit/
    /// circuit-breaker denials included). Read-only over the audit table
    /// `LlmRunner` writes — no scope gate at the service layer; MCP/REST
    /// callers gate this at their own transport layer (cortex:admin /
    /// X-Cortex-Admin-Token) since it exposes operational kill-switch/
    /// circuit-breaker state, not just log content.
    pub async fn llm_invocations_checked(
        &self,
        req: LlmInvocationsRequest,
    ) -> ServiceResult<Vec<crate::db::llm_invocations::LlmInvocationRow>> {
        let limit = req.effective_limit();
        self.run_db("llm_invocations", move |pool| {
            let conn = pool.get()?;
            crate::db::llm_invocations::list_llm_invocations(
                &conn,
                limit,
                req.since.as_deref(),
                req.action.as_deref(),
                req.status.as_deref(),
            )
            .map_err(anyhow::Error::from)
        })
        .await
    }

    /// Send a test notification via configured Apprise destinations.
    ///
    /// Rate-limited to 10/min per actor using an in-memory counter that resets
    /// after 60s of inactivity per actor.
    pub async fn notifications_test_checked(
        &self,
        body: String,
        actor: impl Into<RequestActor>,
        config: &crate::config::NotificationsConfig,
    ) -> ServiceResult<String> {
        self.notifications_test_with_destinations(
            body,
            actor,
            config.apprise_url.clone(),
            config.apprise_urls.clone(),
        )
        .await
    }

    async fn notifications_test_with_destinations(
        &self,
        body: String,
        actor: impl Into<RequestActor>,
        apprise_url: String,
        apprise_urls: Vec<String>,
    ) -> ServiceResult<String> {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        use std::time::Instant;

        const MAX_PER_MIN: u32 = 10;
        let actor = actor.into().display;

        // In-memory rate limiter: actor -> (count, window_start)
        static RATE_LIMITER: OnceLock<Mutex<HashMap<String, (u32, Instant)>>> = OnceLock::new();
        let limiter = RATE_LIMITER.get_or_init(|| Mutex::new(HashMap::new()));

        {
            let mut map = limiter.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            // Evict stale entries (window elapsed) to prevent unbounded map growth.
            map.retain(|_, entry| entry.1.elapsed().as_secs() < 60);
            let entry = map.entry(actor.clone()).or_insert((0, now));
            // Reset window if > 60s has elapsed (belt-and-suspenders after retain)
            if entry.1.elapsed().as_secs() >= 60 {
                *entry = (0, now);
            }
            entry.0 += 1;
            if entry.0 > MAX_PER_MIN {
                return Err(crate::app::ServiceError::InvalidInput(format!(
                    "Rate limit exceeded for actor '{actor}': max {MAX_PER_MIN} test notifications per minute"
                )));
            }
        }

        // Send test notification asynchronously
        let client = crate::notifications::apprise::AppriseClient::new(apprise_url);
        let escaped_body = crate::notifications::apprise::escape_for_notification(&body);
        let result = client
            .notify(
                &apprise_urls,
                "Test Notification",
                &escaped_body,
                crate::notifications::apprise::NotifyType::Info,
            )
            .await;

        match result {
            Ok(resp) => Ok(format!(
                "Test notification sent (status {})",
                resp.status_code
            )),
            Err(e) => Err(crate::app::ServiceError::Internal(anyhow::anyhow!(
                "Apprise delivery failed: {e}"
            ))),
        }
    }

    // -------------------------------------------------------------------------
    // RAG v1 methods
    // -------------------------------------------------------------------------

    pub async fn similar_incidents(
        &self,
        req: SimilarIncidentsRequest,
    ) -> ServiceResult<SimilarIncidentsResponse> {
        let from = parse_optional_timestamp(req.since.as_deref(), "since")?;
        let to = parse_optional_timestamp(req.until.as_deref(), "until")?;
        let severity_min = validate_optional_severity(req.severity_min)?;
        let result = self
            .run_db("similar_incidents", move |pool| {
                db::similar_incidents_clusters(
                    pool,
                    &db::SimilarIncidentsParams {
                        query: req.query,
                        host: req.host,
                        app: req.app,
                        severity_min,
                        since: from,
                        until: to,
                        window_minutes: req.window_minutes,
                        limit: req.limit,
                    },
                )
            })
            .await?;
        Ok(result.into())
    }

    pub async fn incident_context(
        &self,
        req: IncidentContextRequest,
    ) -> ServiceResult<IncidentContextResponse> {
        let to_dt = match req.until.as_deref() {
            Some(until) => parse_required_timestamp(until, "until")?,
            None => chrono::Utc::now(),
        };
        let from_dt = match req.since.as_deref() {
            Some(since) => parse_required_timestamp(since, "since")?,
            None => to_dt - chrono::Duration::hours(1),
        };
        if from_dt > to_dt {
            return Err(ServiceError::InvalidInput(
                "since must not be later than until".into(),
            ));
        }
        let from = rfc3339_z(from_dt);
        let to = rfc3339_z(to_dt);
        let result = self
            .run_db("incident_context", move |pool| {
                db::incident_context_summary(
                    pool,
                    &db::IncidentContextParams {
                        since: from,
                        until: to,
                        host: req.host,
                        app: req.app,
                        query: req.query,
                        severity_min: req.severity_min,
                        limit: req.limit,
                    },
                )
            })
            .await?;
        Ok(result.into())
    }
}

#[cfg(test)]
#[path = "rag_tests.rs"]
mod rag_tests;
