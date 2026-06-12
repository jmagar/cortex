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
                validate_file_tail_path(&add.path)?;
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

fn validate_file_tail_path(path: &str) -> ServiceResult<()> {
    let raw = std::path::Path::new(path);
    if !raw.is_absolute() {
        return Err(ServiceError::InvalidInput(
            "file-tail path must be absolute".into(),
        ));
    }
    let symlink_metadata = std::fs::symlink_metadata(raw).map_err(|err| {
        ServiceError::InvalidInput(format!("file-tail path is not readable: {path}: {err}"))
    })?;
    if symlink_metadata.file_type().is_symlink() {
        return Err(ServiceError::InvalidInput(
            "file-tail path must not be a symlink".into(),
        ));
    }
    if !symlink_metadata.file_type().is_file() {
        return Err(ServiceError::InvalidInput(
            "file-tail path must be a regular file".into(),
        ));
    }

    let canonical = std::fs::canonicalize(raw).map_err(|err| {
        ServiceError::InvalidInput(format!("file-tail path could not be canonicalized: {err}"))
    })?;
    let denied = [
        "/data",
        "/cortex-home",
        "/home/cortex/.ssh",
        "/home/cortex/workspace",
    ];
    if denied
        .iter()
        .any(|root| canonical.starts_with(std::path::Path::new(root)))
    {
        return Err(ServiceError::InvalidInput(
            "file-tail path is under a sensitive cortex mount".into(),
        ));
    }

    let allowed_roots = allowed_file_tail_roots();
    if allowed_roots
        .iter()
        .any(|root| canonical.starts_with(root.as_path()))
    {
        return Ok(());
    }

    Err(ServiceError::InvalidInput(format!(
        "file-tail path is outside allowed roots: {}",
        canonical.display()
    )))
}

fn allowed_file_tail_roots() -> Vec<std::path::PathBuf> {
    std::env::var("CORTEX_FILE_TAIL_ALLOWED_ROOTS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|root| !root.is_empty())
                .map(std::path::PathBuf::from)
                .collect()
        })
        .unwrap_or_else(|| {
            [
                "/file-tail-root",
                "/var/log",
                "/host/var/log",
                "/logs",
                "/mnt",
                "/tmp",
            ]
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect()
        })
}
