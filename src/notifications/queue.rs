//! Outbox queue helpers — thin wrappers around `crate::db::notifications`.
//!
//! These functions acquire a connection from the pool and delegate to the
//! pure-`Connection` functions in `crate::db::notifications`.

use anyhow::Result;

pub use crate::db::notifications::{
    backoff_next_attempt_at, firings_insert, firings_recent, firings_recent_dedup_check,
    outbox_claim_pending, outbox_insert, outbox_mark_dead, outbox_mark_dropped, outbox_mark_sent,
    outbox_schedule_retry, FiringInsertParams, OutboxInsertParams, OutboxRow,
};
use crate::db::DbPool;

/// Claim up to `limit` pending outbox rows from the pool.
pub fn claim_pending(pool: &DbPool, limit: i64) -> Result<Vec<OutboxRow>> {
    let conn = pool.get()?;
    let rows = outbox_claim_pending(&conn, limit)?;
    Ok(rows)
}

/// Insert a notification into the outbox (pool version).
pub fn enqueue(pool: &DbPool, params: &OutboxInsertParams) -> Result<()> {
    let conn = pool.get()?;
    outbox_insert(&conn, params)?;
    Ok(())
}
