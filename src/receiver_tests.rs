use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use super::*;

/// A panicking listener attempt must not kill supervision: the supervisor
/// marks the listener down, backs off, and restarts it (bead syslog-mcp-7f0y).
/// `start_paused` makes the backoff sleeps instant.
#[tokio::test(start_paused = true)]
async fn supervisor_restarts_listener_after_panic() {
    let obs = Arc::new(RuntimeObservability::default());
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_in = Arc::clone(&attempts);
    tokio::spawn(supervise_listener(
        "test_listener",
        Arc::clone(&obs),
        |o, s| o.set_udp_listener_state(s),
        move || {
            let attempts = Arc::clone(&attempts_in);
            async move {
                let n = attempts.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    panic!("poison packet");
                }
                // Second attempt: run forever like a healthy listener.
                std::future::pending::<()>().await;
                Ok(())
            }
        },
    ));

    tokio::time::timeout(Duration::from_secs(300), async {
        while attempts.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // The restarted attempt must be marked alive again.
        while obs.udp_listener_state() != ListenerState::Alive {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("supervisor should restart the listener after a panic");

    assert!(attempts.load(Ordering::SeqCst) >= 2);
    assert!(!obs.any_listener_down());
}

/// A listener that exits with an error is marked down until the restart
/// succeeds; while it keeps failing, `any_listener_down` reports true so
/// /health can fail and Docker can restart the container.
#[tokio::test(start_paused = true)]
async fn supervisor_marks_listener_down_while_failing() {
    let obs = Arc::new(RuntimeObservability::default());
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_in = Arc::clone(&attempts);
    tokio::spawn(supervise_listener(
        "test_listener",
        Arc::clone(&obs),
        |o, s| o.set_tcp_listener_state(s),
        move || {
            let attempts = Arc::clone(&attempts_in);
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("bind failed")
            }
        },
    ));

    // Let several attempts fail.
    tokio::time::timeout(Duration::from_secs(300), async {
        while attempts.load(Ordering::SeqCst) < 3 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("supervisor should keep retrying a failing listener");

    // Between attempts (during backoff) the listener reports down. Sample
    // until we observe it — with paused time this is deterministic enough
    // to catch within the timeout.
    tokio::time::timeout(Duration::from_secs(300), async {
        while obs.tcp_listener_state() != ListenerState::Down {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("failing listener should be observable as down");
    assert!(obs.any_listener_down());
}

/// Listeners that never started (stdio/query-only mode) must not be treated
/// as down by the health probe.
#[test]
fn not_started_listeners_are_not_down() {
    let obs = RuntimeObservability::default();
    assert_eq!(obs.udp_listener_state(), ListenerState::NotStarted);
    assert_eq!(obs.tcp_listener_state(), ListenerState::NotStarted);
    assert!(!obs.any_listener_down());
}
