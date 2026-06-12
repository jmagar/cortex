use crate::app::{ServiceError, ServiceResult};
use crate::file_tail::path_policy::validate_file_tail_path;
use crate::file_tail::{FileTailOp, FileTailRequest, FileTailResponse, FileTailSource};

use super::CortexService;

impl CortexService {
    pub async fn file_tails(&self, req: FileTailRequest) -> ServiceResult<FileTailResponse> {
        req.validate_shape().map_err(ServiceError::InvalidInput)?;
        let registry = self.file_tail_registry.as_ref().ok_or_else(|| {
            ServiceError::InvalidInput("file-tail registry is not mounted".into())
        })?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let mut should_reconcile = false;
        match req.op {
            FileTailOp::List | FileTailOp::Status => {}
            FileTailOp::Add => {
                let add = req.into_add().map_err(ServiceError::InvalidInput)?;
                validate_file_tail_path(&add.path)
                    .map_err(|err| ServiceError::InvalidInput(err.to_string()))?;
                let source =
                    FileTailSource::from_add(add, &now).map_err(ServiceError::InvalidInput)?;
                registry
                    .upsert(source)
                    .map_err(|err| ServiceError::Internal(anyhow::anyhow!(err)))?;
                should_reconcile = true;
            }
            FileTailOp::Remove => {
                let id = req.required_id().map_err(ServiceError::InvalidInput)?;
                registry.remove(id).map_err(map_registry_mutation_error)?;
                should_reconcile = true;
            }
            FileTailOp::Enable => {
                let id = req.required_id().map_err(ServiceError::InvalidInput)?;
                registry
                    .set_enabled(id, true, &now)
                    .map_err(map_registry_mutation_error)?;
                should_reconcile = true;
            }
            FileTailOp::Disable => {
                let id = req.required_id().map_err(ServiceError::InvalidInput)?;
                registry
                    .set_enabled(id, false, &now)
                    .map_err(map_registry_mutation_error)?;
                should_reconcile = true;
            }
        }

        if should_reconcile {
            if let Some(reconcile) = &self.file_tail_reconcile {
                reconcile().map_err(|err| ServiceError::Internal(anyhow::anyhow!(err)))?;
            }
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

fn map_registry_mutation_error(err: anyhow::Error) -> ServiceError {
    let message = err.to_string();
    if message.contains("file tail source not found:") {
        ServiceError::NotFound(message)
    } else {
        ServiceError::Internal(err)
    }
}
