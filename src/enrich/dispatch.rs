//! Dispatcher — picks a parser per `(source_kind, app_name, container_name)`
//! and merges its output onto the entry.
//!
//! Spec: docs/superpowers/specs/2026-05-16-enrichment-framework-design.md §4

use crate::db::LogBatchEntry;
use crate::enrich::Parser;

/// Singleton dispatcher. Built once at startup, then handed to the batch writer.
pub struct EnrichmentPipeline {
    // Populated in Task 16 (parser registration); empty for now.
    #[allow(dead_code)]
    _parsers: Vec<&'static dyn Parser>,
}

impl EnrichmentPipeline {
    /// Build the dispatcher with the V1 parser set. For now empty.
    pub fn new() -> Self {
        Self { _parsers: Vec::new() }
    }

    /// Dispatch and merge. No-op while the parser table is empty (Task 16 fills it).
    pub fn dispatch(&self, _entry: &mut LogBatchEntry) {
        // No parsers registered yet — leave entry as-is.
    }
}

impl Default for EnrichmentPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod dispatch_tests;
