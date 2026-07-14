use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ContainerMeta {
    pub id: String,
    pub name: String,
    pub image: String,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
}

impl ContainerMeta {
    pub(super) fn from_summary(summary: bollard::models::ContainerSummary) -> Option<Self> {
        let id = summary.id?;
        let name = summary
            .names
            .and_then(|names| names.into_iter().next())
            .map(|name| name.trim_start_matches('/').to_string())
            .unwrap_or_else(|| id.chars().take(12).collect());
        let labels: HashMap<String, String> = summary.labels.unwrap_or_default();
        Some(Self {
            id,
            name,
            image: summary.image.unwrap_or_default(),
            compose_project: labels.get("com.docker.compose.project").cloned(),
            compose_service: labels.get("com.docker.compose.service").cloned(),
        })
    }

    /// Flat, slash-free app name: `compose_service` when present, else the
    /// container name. Canonical service identity is resolved separately
    /// from structured agent-docker metadata (see `src/db/entity_resolution`)
    /// — this label is a display/search string only, never parsed for
    /// identity. A slash-triplet shape is deliberately avoided: the resolver
    /// classifies multi-slash app labels as a legacy shape and excludes them
    /// from graph projection (see `classify_legacy_shape` in
    /// `src/db/entity_resolution/vocab.rs`).
    pub(super) fn app_name(&self) -> String {
        self.compose_service
            .clone()
            .unwrap_or_else(|| self.name.clone())
    }

    pub(super) fn short_id(&self) -> String {
        self.id.chars().take(12).collect()
    }
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
