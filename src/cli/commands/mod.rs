//! Per-subcommand parser modules.
//!
//! Each module contains the `parse_*` functions for one CLI surface.
//! The parent `cli.rs` dispatches to these modules via the top-level
//! `parse_command` match arm.
//!
//! # Migration status (Q-C1)
//!
//! cli.rs was 5,005 LOC at the start of this work. Only `args.rs` had been
//! extracted. This change begins populating `src/cli/commands/` with leaf
//! subcommands. The hand-rolled `FlagCursor` parser and helpers in cli.rs
//! are shared via `pub(crate)` re-exports until a full clap-derive migration
//! is done.
//!
//! Extracted so far:
//! - `sig` — error-signature commands (`list`, `ack`, `unack`)
//! - `notify` — notification commands (`recent`)
//!
//! Remaining (each is ~50-100 LOC of parse functions):
//! db, setup, compose, ai, config, source-ips, timeline, patterns, etc.

pub mod notify;
pub mod sig;
