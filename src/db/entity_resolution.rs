#![allow(dead_code)]

//! Canonical entity resolution: key grammar, observations, and deterministic
//! resolver decisions for the investigation graph.
//!
//! This module owns the hard-break canonical service identity contract:
//! `logical_service:plex` for logical identity and
//! `service_instance:tootie/plex` for host-scoped deployment topology.
//! Legacy nested shapes (`tootie:plex`, `tootie:plex:plex`, `plex/plex/plex`)
//! are classified for rejection, never normalized.

pub mod vocab;

pub use vocab::*;

#[cfg(test)]
#[path = "entity_resolution_tests.rs"]
mod tests;
