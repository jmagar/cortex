use crate::inventory::schema::{
    ArtifactRef, CollectionError, ComposeProject, InventoryNode, InventoryService, MediaService,
    NetworkSegment, ProjectRepo, ReverseProxyRoute, StorageSummary,
};

#[derive(Debug, Default)]
pub struct CollectorOutput {
    pub name: &'static str,
    pub nodes: Vec<InventoryNode>,
    pub services: Vec<InventoryService>,
    pub compose_projects: Vec<ComposeProject>,
    pub reverse_proxies: Vec<ReverseProxyRoute>,
    pub networks: Vec<NetworkSegment>,
    pub storage: Vec<StorageSummary>,
    pub media_services: Vec<MediaService>,
    pub projects: Vec<ProjectRepo>,
    pub artifacts: Vec<ArtifactRef>,
    pub errors: Vec<CollectionError>,
    pub warnings: Vec<String>,
}

impl CollectorOutput {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            ..Self::default()
        }
    }

    pub fn warn(&mut self, phase: &str, message: impl Into<String>) {
        let message = message.into();
        self.warnings.push(message.clone());
        self.errors.push(CollectionError {
            collector: self.name.to_string(),
            phase: phase.to_string(),
            severity: "warning".to_string(),
            message,
            elapsed_ms: 0,
            truncated: false,
        });
    }

    /// Record an informational collector skip without presenting it as a map
    /// collection error. Optional integrations that have not been configured
    /// are expected absence, not failed collection attempts.
    pub fn skip(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }
}
