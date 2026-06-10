//! Error detection subsystem.
//!
//! Provides background scanning of error-severity log rows, normalizing them
//! into signature templates and surfacing unaddressed repeating patterns via
//! MCP actions.

pub(crate) mod scanner;

// Message normalization lives in the leaf `crate::normalize` module so the
// db layer can use it without depending upward on the service layer
// (full-review AM1). Re-exported here to keep existing call paths stable.
pub(crate) use crate::normalize;
pub(crate) use crate::normalize::NORMALIZER_VERSION;
pub(crate) use scanner::run_error_scan;
