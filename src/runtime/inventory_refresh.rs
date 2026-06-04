use std::time::Instant;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::background_interval;

/// Default cadence for refreshing the private homelab inventory cache consumed
/// by `cortex map`. Set `CORTEX_INVENTORY_REFRESH_INTERVAL_SECS=0` to disable.
const INVENTORY_REFRESH_INTERVAL_SECS: u64 = 300;

pub fn spawn(token: CancellationToken) -> Option<JoinHandle<()>> {
    let interval_secs = inventory_refresh_interval_secs();
    if interval_secs == 0 {
        tracing::info!("inventory_refresh: disabled");
        return None;
    }
    Some(tokio::spawn(async move {
        let mut interval = background_interval(tokio::time::Duration::from_secs(interval_secs));
        let mut eager = true;
        loop {
            if eager {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        tracing::debug!("inventory_refresh: cancelled before first refresh");
                        break;
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(15)) => {}
                }
                eager = false;
            } else {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        tracing::debug!("inventory_refresh: cooperative shutdown");
                        break;
                    }
                    _ = interval.tick() => {}
                }
            }
            let started = Instant::now();
            match crate::inventory::refresh_inventory(crate::inventory::InventoryConfig::from_env())
                .await
            {
                Ok(report) => tracing::info!(
                    status = %report.status,
                    run_id = %report.run_id,
                    warnings = report.warnings.len(),
                    elapsed_ms = started.elapsed().as_millis(),
                    "inventory_refresh: cache refresh complete"
                ),
                Err(error) => tracing::warn!(
                    %error,
                    elapsed_ms = started.elapsed().as_millis(),
                    "inventory_refresh: cache refresh failed"
                ),
            }
        }
    }))
}

fn inventory_refresh_interval_secs() -> u64 {
    std::env::var("CORTEX_INVENTORY_REFRESH_INTERVAL_SECS")
        .ok()
        .as_deref()
        .and_then(parse_inventory_refresh_interval_secs)
        .unwrap_or(INVENTORY_REFRESH_INTERVAL_SECS)
}

fn parse_inventory_refresh_interval_secs(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok()
}

#[cfg(test)]
#[path = "inventory_refresh_tests.rs"]
mod tests;
