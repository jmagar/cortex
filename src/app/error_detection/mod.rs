//! Error detection subsystem.
//!
//! Provides background scanning of error-severity log rows, normalizing them
//! into signature templates and surfacing unaddressed repeating patterns via
//! MCP actions.

pub(crate) mod normalize;
pub(crate) mod scanner;

pub(crate) use normalize::NORMALIZER_VERSION;
pub(crate) use scanner::run_error_scan;
