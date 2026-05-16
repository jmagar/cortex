//! Dispatcher stub — full implementation lands in Task 6.
#![allow(dead_code)]

use crate::db::LogBatchEntry;

/// Singleton dispatcher. Built once at startup, populated in Task 6.
pub struct EnrichmentPipeline;

impl EnrichmentPipeline {
    pub fn new() -> Self { Self }
    pub fn dispatch(&self, _entry: &mut LogBatchEntry) {}
}

impl Default for EnrichmentPipeline {
    fn default() -> Self { Self::new() }
}
