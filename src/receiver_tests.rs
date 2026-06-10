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

/// A listener that runs stably past `LISTENER_STABLE_RUN` resets the backoff
/// ladder. After the stable run ends, the next restart uses `LISTENER_BACKOFF_INITIAL`
/// rather than the doubled value it would have after the first failure.
#[tokio::test(start_paused = true)]
async fn supervisor_resets_backoff_after_stable_run() {
    let obs = Arc::new(RuntimeObservability::default());
    let attempts = Arc::new(AtomicU32::new(0));
    // Record the wall-clock instant at which each attempt *exits* (fails).
    // This gives us the reference point for measuring how long the subsequent
    // backoff sleep lasts — independent of how long the attempt itself ran.
    let exit_times: Arc<parking_lot::Mutex<Vec<tokio::time::Instant>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    // Record when attempt 3 *starts* so we can compute the gap from attempt 2's exit.
    let attempt3_start: Arc<parking_lot::Mutex<Option<tokio::time::Instant>>> =
        Arc::new(parking_lot::Mutex::new(None));

    let attempts_in = Arc::clone(&attempts);
    let exits_in = Arc::clone(&exit_times);
    let start3_in = Arc::clone(&attempt3_start);

    tokio::spawn(supervise_listener(
        "test_listener",
        Arc::clone(&obs),
        |o, s| o.set_udp_listener_state(s),
        move || {
            let attempts = Arc::clone(&attempts_in);
            let exits = Arc::clone(&exits_in);
            let start3 = Arc::clone(&start3_in);
            async move {
                let n = attempts.fetch_add(1, Ordering::SeqCst);
                match n {
                    0 => {
                        // First attempt: fail immediately → triggers LISTENER_BACKOFF_INITIAL wait.
                        exits.lock().push(tokio::time::Instant::now());
                        anyhow::bail!("first fail")
                    }
                    1 => {
                        // Second attempt: survive past LISTENER_STABLE_RUN, then fail.
                        // This should reset the backoff back to LISTENER_BACKOFF_INITIAL.
                        tokio::time::sleep(LISTENER_STABLE_RUN + Duration::from_secs(1)).await;
                        exits.lock().push(tokio::time::Instant::now());
                        anyhow::bail!("stable then fail")
                    }
                    _ => {
                        // Third attempt: record start time, then run forever.
                        *start3.lock() = Some(tokio::time::Instant::now());
                        std::future::pending::<()>().await;
                        Ok(())
                    }
                }
            }
        },
    ));

    // Wait for three attempts to start (attempt 3 records its start).
    tokio::time::timeout(Duration::from_secs(300), async {
        while attempt3_start.lock().is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("supervisor should reach three attempts");

    let exits = exit_times.lock();
    assert_eq!(exits.len(), 2, "expected exit times for attempts 0 and 1");
    let start3 = attempt3_start.lock().unwrap();

    // The gap from attempt 1's exit to attempt 3's start is the backoff sleep.
    // After a stable run, backoff resets to LISTENER_BACKOFF_INITIAL (1 s).
    // Without the reset it would be LISTENER_BACKOFF_INITIAL * 2 (2 s).
    // Allow a 500 ms scheduling margin.
    let gap = start3.duration_since(exits[1]);
    assert!(
        gap <= LISTENER_BACKOFF_INITIAL + Duration::from_millis(500),
        "backoff after stable run should be ~LISTENER_BACKOFF_INITIAL ({:?}), got {:?}",
        LISTENER_BACKOFF_INITIAL,
        gap
    );
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
