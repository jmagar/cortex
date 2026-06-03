use super::*;

impl CortexService {
    // ---- Error detection MCP actions ----------------------------------------

    pub async fn unaddressed_errors(
        &self,
        req: models::UnaddressedErrorsRequest,
    ) -> ServiceResult<models::UnaddressedErrorsResponse> {
        let limit = req.limit.unwrap_or(50) as i64;
        let include_acked = req.include_acknowledged.unwrap_or(false);
        self.run_db("unaddressed_errors", move |pool| {
            let rows = crate::db::error_signatures::read_unaddressed(pool, limit, include_acked)?;
            let signatures = rows
                .into_iter()
                .map(|r| models::ErrorSignatureEntry {
                    signature_hash: r.signature_hash,
                    template: r.template,
                    sample_message: r.sample_message,
                    severity: r.severity,
                    sample_hostname: r.sample_hostname,
                    sample_app_name: r.sample_app_name,
                    first_seen_at: r.first_seen_at,
                    last_seen_at: r.last_seen_at,
                    total_count: r.total_count,
                    count_last_1h: r.count_last_1h,
                    acknowledged_at: r.acknowledged_at,
                })
                .collect();
            Ok(models::UnaddressedErrorsResponse { signatures })
        })
        .await
    }

    pub async fn ack_error(
        &self,
        req: models::AckErrorRequest,
        actor: impl Into<RequestActor>,
    ) -> ServiceResult<models::AckErrorResponse> {
        if let Some(ref n) = req.notes {
            if n.len() > 4096 {
                return Err(ServiceError::InvalidInput(
                    "notes exceeds 4096 chars".into(),
                ));
            }
        }
        let hash = req.signature_hash.clone();
        let notes = req.notes.clone();
        let actor = actor.into();
        let actor_owned = actor.display.clone();
        // Check it exists first
        let h = hash.clone();
        let exists = self
            .run_db("ack_error.exists", move |pool| {
                Ok(crate::db::error_signatures::read_signature_by_hash(
                    pool,
                    &h,
                    crate::app::error_detection::NORMALIZER_VERSION,
                )?
                .is_some())
            })
            .await?;
        if !exists {
            return Err(ServiceError::NotFound(format!(
                "Signature '{}' not found",
                hash
            )));
        }
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let now_clone = now.clone();
        let actor_clone = actor_owned.clone();
        let hash_clone = hash.clone();
        self.run_db("ack_error.commit", move |pool| {
            let mut conn = pool.get()?;
            let _write_guard = crate::db::write_lock();
            let tx = conn.transaction()?;
            crate::db::error_signatures::record_ack_event(
                &tx,
                &hash_clone,
                crate::app::error_detection::NORMALIZER_VERSION,
                "ack",
                &actor_clone,
                notes.as_deref(),
            )?;
            crate::db::error_signatures::update_ack_projection(
                &tx,
                &hash_clone,
                crate::app::error_detection::NORMALIZER_VERSION,
                Some(&now_clone),
                Some(&actor_clone),
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
        Ok(models::AckErrorResponse {
            signature_hash: hash,
            acknowledged_at: now,
            actor: actor_owned,
        })
    }

    pub async fn unack_error(
        &self,
        req: models::UnackErrorRequest,
        actor: impl Into<RequestActor>,
    ) -> ServiceResult<models::UnackErrorResponse> {
        if let Some(ref r) = req.reason {
            if r.len() > 4096 {
                return Err(ServiceError::InvalidInput(
                    "reason exceeds 4096 chars".into(),
                ));
            }
        }
        let hash = req.signature_hash.clone();
        let reason = req.reason.clone();
        let actor = actor.into();
        let actor_owned = actor.display.clone();
        // Check it exists first
        let h = hash.clone();
        let exists = self
            .run_db("unack_error.exists", move |pool| {
                Ok(crate::db::error_signatures::read_signature_by_hash(
                    pool,
                    &h,
                    crate::app::error_detection::NORMALIZER_VERSION,
                )?
                .is_some())
            })
            .await?;
        if !exists {
            return Err(ServiceError::NotFound(format!(
                "Signature '{}' not found",
                hash
            )));
        }
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let actor_clone = actor_owned.clone();
        let hash_clone = hash.clone();
        self.run_db("unack_error.commit", move |pool| {
            let mut conn = pool.get()?;
            let _write_guard = crate::db::write_lock();
            let tx = conn.transaction()?;
            crate::db::error_signatures::record_ack_event(
                &tx,
                &hash_clone,
                crate::app::error_detection::NORMALIZER_VERSION,
                "unack",
                &actor_clone,
                reason.as_deref(),
            )?;
            crate::db::error_signatures::update_ack_projection(
                &tx,
                &hash_clone,
                crate::app::error_detection::NORMALIZER_VERSION,
                None,
                None,
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
        Ok(models::UnackErrorResponse {
            signature_hash: hash,
            unacked_at: now,
            actor: actor_owned,
        })
    }
}
