use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::inventory::redaction::RedactedArtifact;
use crate::inventory::schema::ArtifactRef;

#[derive(Debug, Clone)]
pub struct InventoryPaths {
    pub root: PathBuf,
    pub raw_dir: PathBuf,
    pub normalized_dir: PathBuf,
    pub normalized_json: PathBuf,
    pub collection_state_json: PathBuf,
    pub lock_file: PathBuf,
}

impl InventoryPaths {
    pub fn new(root: PathBuf) -> Self {
        let raw_dir = root.join("raw");
        let normalized_dir = root.join("normalized");
        Self {
            normalized_json: normalized_dir.join("homelab.json"),
            collection_state_json: root.join("collection-state.json"),
            lock_file: root.join("refresh.lock"),
            root,
            raw_dir,
            normalized_dir,
        }
    }

    pub fn ensure_private_dirs(&self) -> Result<()> {
        ensure_private_dir(&self.root)?;
        ensure_private_dir(&self.raw_dir)?;
        ensure_private_dir(&self.normalized_dir)?;
        Ok(())
    }

    pub fn raw_artifact_path(&self, run_id: &str, id: &str) -> PathBuf {
        self.raw_dir.join(run_id).join(format!("{id}.txt"))
    }
}

pub struct RefreshLock {
    path: PathBuf,
}

impl RefreshLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            ensure_private_dir(parent)?;
        }
        reject_symlink(path)?;
        remove_stale_lock(path)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| {
                format!(
                    "inventory refresh already running or stale lock exists: {}",
                    path.display()
                )
            })?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for RefreshLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn write_json_private<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let body = serde_json::to_string_pretty(value)?;
    write_private_atomic(path, body.as_bytes())
}

pub fn write_artifact(
    paths: &InventoryPaths,
    run_id: &str,
    artifact_id: &str,
    artifact: &RedactedArtifact,
    mut reference: ArtifactRef,
) -> Result<ArtifactRef> {
    let path = paths.raw_artifact_path(run_id, artifact_id);
    write_private_atomic(&path, artifact.body().as_bytes())?;
    reference.cache_path = path.display().to_string();
    reference.redaction = artifact.status();
    reference.byte_len = artifact.body().len();
    reference.truncated = artifact.truncated();
    Ok(reference)
}

pub fn read_json<T: for<'de> serde::Deserialize<'de>>(path: &Path) -> Result<T> {
    reject_symlink(path)?;
    let body = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&body).map_err(|error| anyhow!("parse {}: {error}", path.display()))
}

fn write_private_atomic(path: &Path, body: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    reject_symlink(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path has no parent: {}", path.display()))?;
    let tmp = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("inventory"),
        std::process::id()
    ));
    reject_symlink(&tmp)?;
    let write_result = (|| -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options
            .open(&tmp)
            .with_context(|| format!("create {}", tmp.display()))?;
        file.write_all(body)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    if let Err(error) = chmod_private_file(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    fs::rename(&tmp, path)
        .inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    chmod_private_file(path)?;
    Ok(())
}

fn remove_stale_lock(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    let Ok(modified) = metadata.modified() else {
        return Ok(());
    };
    if modified.elapsed().unwrap_or_default() > Duration::from_secs(6 * 60 * 60) {
        fs::remove_file(path).with_context(|| format!("remove stale lock {}", path.display()))?;
    }
    Ok(())
}

pub fn ensure_private_dir(path: &Path) -> Result<()> {
    reject_symlink(path)?;
    fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("chmod 0700 {}", path.display()))?;
    }
    Ok(())
}

fn chmod_private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    Ok(())
}

pub fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("inventory path must not be a symlink: {}", path.display())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
