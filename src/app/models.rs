use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::app::incident_findings;
use crate::db;

mod ai_incidents;
mod ai_inventory;
mod ai_sessions;
mod context;
mod core;
mod graph;
mod investigation;
mod log_query;
mod ops;
mod rag;
mod stats;
mod surface;

pub use ai_incidents::*;
pub use ai_inventory::*;
pub use ai_sessions::*;
pub use context::*;
pub use core::*;
pub use graph::*;
pub use investigation::*;
pub use log_query::*;
pub use ops::*;
pub use rag::*;
pub use stats::*;
pub use surface::*;

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
