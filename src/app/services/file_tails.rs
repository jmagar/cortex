use crate::app::{ServiceError, ServiceResult};
use crate::file_tail::{FileTailOp, FileTailRequest, FileTailResponse, FileTailSource};

use super::CortexService;

impl CortexService {
    pub async fn file_tails(&self, req: FileTailRequest) -> ServiceResult<FileTailResponse> {
        req.validate().map_err(ServiceError::InvalidInput)?;
        let registry = self.file_tail_registry.as_ref().ok_or_else(|| {
            ServiceError::InvalidInput("file-tail registry is not mounted".into())
        })?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        match req.op {
            FileTailOp::List | FileTailOp::Status => {}
            FileTailOp::Add => {
                let add = req.into_add().map_err(ServiceError::InvalidInput)?;
                registry
                    .upsert(FileTailSource::from_add(add, &now))
                    .map_err(|err| ServiceError::Internal(anyhow::anyhow!(err)))?;
            }
            FileTailOp::Remove => {
                registry
                    .remove(req.id.as_deref().expect("validated id"))
                    .map_err(|err| ServiceError::Internal(anyhow::anyhow!(err)))?;
            }
            FileTailOp::Enable => {
                registry
                    .set_enabled(req.id.as_deref().expect("validated id"), true, &now)
                    .map_err(|err| ServiceError::InvalidInput(err.to_string()))?;
            }
            FileTailOp::Disable => {
                registry
                    .set_enabled(req.id.as_deref().expect("validated id"), false, &now)
                    .map_err(|err| ServiceError::InvalidInput(err.to_string()))?;
            }
        }

        if let Some(reconcile) = &self.file_tail_reconcile {
            reconcile().map_err(|err| ServiceError::Internal(anyhow::anyhow!(err)))?;
        }

        let sources = registry
            .list()
            .map_err(|err| ServiceError::Internal(anyhow::anyhow!(err)))?;
        let statuses = self
            .file_tail_statuses
            .as_ref()
            .map(|statuses| statuses())
            .unwrap_or_default();
        Ok(FileTailResponse { sources, statuses })
    }
}
