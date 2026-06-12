use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use parking_lot::Mutex;

use super::models::FileTailSource;

#[derive(Debug)]
pub(crate) struct FileTailRegistry {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileTailRegistry {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub(crate) fn path_from_storage_db(db_path: &Path) -> PathBuf {
        db_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("file-tails.json")
    }

    pub(crate) fn list(&self) -> Result<Vec<FileTailSource>> {
        let _guard = self.lock.lock();
        self.read_locked()
    }

    pub(crate) fn upsert(&self, source: FileTailSource) -> Result<()> {
        let _guard = self.lock.lock();
        let mut sources = self.read_locked()?;
        sources.retain(|existing| existing.id != source.id);
        sources.push(source);
        sources.sort_by(|a, b| a.id.cmp(&b.id));
        self.write_locked(&sources)
    }

    pub(crate) fn remove(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock();
        let mut sources = self.read_locked()?;
        sources.retain(|existing| existing.id != id);
        self.write_locked(&sources)
    }

    pub(crate) fn set_enabled(&self, id: &str, enabled: bool, now: &str) -> Result<()> {
        let _guard = self.lock.lock();
        let mut sources = self.read_locked()?;
        let source = sources
            .iter_mut()
            .find(|source| source.id == id)
            .with_context(|| format!("file tail source not found: {id}"))?;
        source.enabled = enabled;
        source.updated_at = now.to_string();
        self.write_locked(&sources)
    }

    pub(crate) fn update_checkpoint(
        &self,
        id: &str,
        dev: u64,
        ino: u64,
        offset: u64,
        now: &str,
    ) -> Result<()> {
        let _guard = self.lock.lock();
        let mut sources = self.read_locked()?;
        let source = sources
            .iter_mut()
            .find(|source| source.id == id)
            .with_context(|| format!("file tail source not found: {id}"))?;
        source.checkpoint_dev = Some(dev);
        source.checkpoint_ino = Some(ino);
        source.checkpoint_offset = Some(offset);
        source.updated_at = now.to_string();
        self.write_locked(&sources)
    }

    fn read_locked(&self) -> Result<Vec<FileTailSource>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&self.path)
            .with_context(|| format!("read {}", self.path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parse {}", self.path.display()))
    }

    fn write_locked(&self, sources: &[FileTailSource]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(sources)?;
        std::fs::write(&tmp, body).with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("replace {}", self.path.display()))?;
        Ok(())
    }
}
