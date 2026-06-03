use super::*;

pub async fn run_compose_status() -> ServiceResult<crate::compose::ComposeStatus> {
    static COMPOSE_DIAGNOSTICS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let permit = COMPOSE_DIAGNOSTICS
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| ServiceError::Busy(format!("compose diagnostics limiter closed: {e}")))?;
    let service = crate::compose::ComposeService::new(
        crate::compose::CliDockerInspect,
        crate::compose::ProcessRunner,
        crate::compose::ComposeDefaults::default(),
    );
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        service.status(&crate::compose::ComposeTarget::default())
    })
    .await
    .map_err(|e| anyhow::anyhow!("compose status task failed: {e}"))?
    .map_err(ServiceError::from)
}
