use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

pub(crate) fn validate_file_tail_path(path: &str) -> Result<()> {
    let raw = Path::new(path);
    if !raw.is_absolute() {
        bail!("file-tail path must be absolute");
    }
    let symlink_metadata = std::fs::symlink_metadata(raw)
        .map_err(|err| anyhow::anyhow!("file-tail path is not readable: {path}: {err}"))?;
    if symlink_metadata.file_type().is_symlink() {
        bail!("file-tail path must not be a symlink");
    }
    if !symlink_metadata.file_type().is_file() {
        bail!("file-tail path must be a regular file");
    }

    let canonical = std::fs::canonicalize(raw)
        .map_err(|err| anyhow::anyhow!("file-tail path could not be canonicalized: {err}"))?;
    let denied = [
        "/data",
        "/cortex-home",
        "/home/cortex/.ssh",
        "/home/cortex/workspace",
    ];
    if denied
        .iter()
        .any(|root| canonical.starts_with(Path::new(root)))
    {
        bail!("file-tail path is under a sensitive cortex mount");
    }

    let allowed_roots = allowed_file_tail_roots();
    if allowed_roots.iter().any(|root| canonical.starts_with(root)) {
        return Ok(());
    }

    bail!(
        "file-tail path is outside allowed roots: {}",
        canonical.display()
    );
}

fn allowed_file_tail_roots() -> Vec<PathBuf> {
    std::env::var("CORTEX_FILE_TAIL_ALLOWED_ROOTS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|root| !root.is_empty())
                .map(PathBuf::from)
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
            .map(PathBuf::from)
            .collect()
        })
}
