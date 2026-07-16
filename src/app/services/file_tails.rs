use crate::app::{ServiceError, ServiceResult};
use crate::filetail::path_policy::validate_file_tail_path;
use crate::filetail::{FileTailOp, FileTailRequest, FileTailResponse, FileTailSource};

use super::CortexService;

impl CortexService {
    pub async fn file_tails(&self, req: FileTailRequest) -> ServiceResult<FileTailResponse> {
        req.validate_shape().map_err(ServiceError::InvalidInput)?;
        let registry = self.file_tail_registry.as_ref().ok_or_else(|| {
            ServiceError::InvalidInput("file-tail registry is not mounted".into())
        })?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let mutation_reconcile = || {
            self.file_tail_reconcile.as_ref().ok_or_else(|| {
                ServiceError::InvalidInput(
                    "file-tail mutations require the long-running server; query-only mode cannot manage tailers".into(),
                )
            })
        };
        let mut should_reconcile = false;
        match req.op {
            FileTailOp::List | FileTailOp::Status => {}
            FileTailOp::Add => {
                let reconcile = mutation_reconcile()?;
                let add = req.into_add().map_err(ServiceError::InvalidInput)?;
                validate_file_tail_path(&add.path)
                    .map_err(|err| ServiceError::InvalidInput(err.to_string()))?;
                let source =
                    FileTailSource::from_add(add, &now).map_err(ServiceError::InvalidInput)?;
                registry
                    .mutate_with_reconcile(
                        |registry| registry.insert_if_absent(source),
                        || reconcile(),
                    )
                    .map_err(map_registry_mutation_error)?;
                should_reconcile = true;
            }
            FileTailOp::Remove => {
                let reconcile = mutation_reconcile()?;
                let id = req.required_id().map_err(ServiceError::InvalidInput)?;
                registry
                    .mutate_with_reconcile(|registry| registry.remove(id), || reconcile())
                    .map_err(map_registry_mutation_error)?;
                should_reconcile = true;
            }
            FileTailOp::Enable => {
                let reconcile = mutation_reconcile()?;
                let id = req.required_id().map_err(ServiceError::InvalidInput)?;
                registry
                    .mutate_with_reconcile(
                        |registry| registry.set_enabled(id, true, &now),
                        || reconcile(),
                    )
                    .map_err(map_registry_mutation_error)?;
                should_reconcile = true;
            }
            FileTailOp::Disable => {
                let reconcile = mutation_reconcile()?;
                let id = req.required_id().map_err(ServiceError::InvalidInput)?;
                registry
                    .mutate_with_reconcile(
                        |registry| registry.set_enabled(id, false, &now),
                        || reconcile(),
                    )
                    .map_err(map_registry_mutation_error)?;
                should_reconcile = true;
            }
        }

        let sources = registry.list().map_err(|err| {
            if should_reconcile {
                ServiceError::Internal(anyhow::anyhow!(
                    "file-tail mutation was committed, but refresh failed: {err}"
                ))
            } else {
                ServiceError::Internal(anyhow::anyhow!(err))
            }
        })?;
        let statuses = self
            .file_tail_statuses
            .as_ref()
            .map(|statuses| statuses())
            .unwrap_or_default();
        Ok(FileTailResponse { sources, statuses })
    }

    pub fn file_tail_statuses_snapshot(&self) -> Vec<crate::filetail::FileTailStatus> {
        self.file_tail_statuses
            .as_ref()
            .map(|statuses| statuses())
            .unwrap_or_default()
    }
}

fn map_registry_mutation_error(err: anyhow::Error) -> ServiceError {
    let message = err.to_string();
    if message.contains("file tail source already exists:") {
        ServiceError::InvalidInput(message)
    } else if message.contains("file tail source not found:") {
        ServiceError::NotFound(message)
    } else {
        ServiceError::Internal(err)
    }
}

#[cfg(test)]
#[path = "file_tails_tests.rs"]
mod tests;
