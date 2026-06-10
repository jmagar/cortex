use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::io;

use crate::inventory::config::InventoryConfig;
use crate::inventory::schema::{CollectionState, HomelabInventory};
use crate::inventory::storage::{InventoryPaths, read_json};

#[derive(Debug, Clone, Serialize)]
pub struct InventoryCacheStatus {
    pub root: String,
    pub normalized_path: String,
    pub collection_state_path: String,
    pub status: String,
    pub generated_at: Option<String>,
    pub age_seconds: Option<i64>,
    pub is_stale: bool,
    pub warnings: Vec<String>,
    pub collection_state: Option<CollectionState>,
}

#[derive(Deserialize)]
struct InventoryCacheMetadata {
    generated_at: String,
    freshness: InventoryFreshnessMetadata,
}

#[derive(Deserialize)]
struct InventoryFreshnessMetadata {
    stale_after_secs: usize,
}

pub fn read_inventory_cache(config: &InventoryConfig) -> Result<HomelabInventory> {
    let paths = InventoryPaths::new(config.root.clone());
    read_json(&paths.normalized_json)
}

pub fn is_not_found_error(error: &anyhow::Error) -> bool {
    error
        .root_cause()
        .downcast_ref::<io::Error>()
        .is_some_and(|io_error| io_error.kind() == io::ErrorKind::NotFound)
}

pub fn inventory_status(config: &InventoryConfig) -> InventoryCacheStatus {
    let paths = InventoryPaths::new(config.root.clone());
    let mut warnings = Vec::new();
    let mut status = "missing".to_string();
    let mut generated_at = None;
    let mut age_seconds = None;
    let mut is_stale = true;

    let collection_state = match read_json::<CollectionState>(&paths.collection_state_json) {
        Ok(state) => Some(state),
        Err(error) => {
            warnings.push(format!("collection-state unavailable: {error}"));
            None
        }
    };

    match read_json::<InventoryCacheMetadata>(&paths.normalized_json) {
        Ok(metadata) => {
            generated_at = Some(metadata.generated_at.clone());
            if let Ok(ts) = DateTime::parse_from_rfc3339(&metadata.generated_at) {
                let age = Utc::now().signed_duration_since(ts.with_timezone(&Utc));
                age_seconds = Some(age.num_seconds().max(0));
                let stale_after =
                    i64::try_from(metadata.freshness.stale_after_secs).unwrap_or(i64::MAX);
                is_stale = age > Duration::seconds(stale_after);
            }
            status = if is_stale {
                "available_stale".to_string()
            } else {
                "available".to_string()
            };
        }
        Err(error) if paths.normalized_json.exists() => {
            status = "corrupt".to_string();
            warnings.push(format!("normalized cache unreadable: {error}"));
        }
        Err(_) => {}
    }

    InventoryCacheStatus {
        root: paths.root.display().to_string(),
        normalized_path: paths.normalized_json.display().to_string(),
        collection_state_path: paths.collection_state_json.display().to_string(),
        status,
        generated_at,
        age_seconds,
        is_stale,
        warnings,
        collection_state,
    }
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
