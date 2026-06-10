use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;

use chrono::Utc;
use serde::Serialize;

/// Lifecycle state of a syslog listener task, stored as an `AtomicU8`.
///
/// `NotStarted` (the default) covers stdio/query-only mode and tests where the
/// listeners never run — health checks must NOT treat it as a failure.
/// `Down` means the listener future exited (error or panic) and ingestion on
/// that transport is not currently receiving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerState {
    NotStarted = 0,
    Alive = 1,
    Down = 2,
}

impl ListenerState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Alive,
            2 => Self::Down,
            _ => Self::NotStarted,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Alive => "alive",
            Self::Down => "down",
        }
    }
}

#[derive(Debug, Default)]
pub struct RuntimeObservability {
    syslog_udp_listener_state: AtomicU8,
    syslog_tcp_listener_state: AtomicU8,
    syslog_udp_packets_received: AtomicU64,
    syslog_udp_bytes_received: AtomicU64,
    syslog_tcp_connections_accepted: AtomicU64,
    syslog_tcp_connections_active: AtomicU64,
    syslog_tcp_connections_closed: AtomicU64,
    syslog_tcp_connections_rejected: AtomicU64,
    syslog_tcp_lines_received: AtomicU64,
    syslog_tcp_bytes_received: AtomicU64,
    syslog_tcp_lines_dropped_oversize: AtomicU64,
    syslog_write_channel_full_transitions: AtomicU64,
    syslog_udp_packets_dropped_queue_full: AtomicU64,
    syslog_tcp_lines_dropped_queue_full: AtomicU64,
    docker_ingest_events_received: AtomicU64,
    docker_ingest_log_entries_received: AtomicU64,
    docker_ingest_parse_errors: AtomicU64,
    docker_ingest_stream_reconnects: AtomicU64,
    docker_ingest_stream_failures: AtomicU64,
    docker_ingest_tasks_spawned: AtomicU64,
    docker_ingest_host_streams_active: AtomicU64,
    docker_ingest_container_streams_active: AtomicU64,
    remote_docker_event_stream_failures: AtomicU64,
    ingest_entries_enqueued: AtomicU64,
    ingest_enqueue_errors: AtomicU64,
    ingest_queue_depth: AtomicUsize,
    ingest_queue_capacity: AtomicUsize,
    writer_batches_flushed: AtomicU64,
    writer_logs_written: AtomicU64,
    writer_flush_failures: AtomicU64,
    writer_logs_retained: AtomicU64,
    writer_logs_discarded: AtomicU64,
    writer_storage_blocked: AtomicBool,
    last_ingest_at: Mutex<Option<String>>,
    last_write_at: Mutex<Option<String>>,
    last_error_at: Mutex<Option<String>>,
    last_docker_ingest_event_at: Mutex<Option<String>>,
    last_docker_ingest_log_at: Mutex<Option<String>>,
    last_docker_ingest_error_at: Mutex<Option<String>>,
    last_remote_docker_event_stream_error_at: Mutex<Option<String>>,
    last_remote_docker_event_stream_error: Mutex<Option<String>>,
    /// Last-tick timestamps for the ~12 background maintenance tasks, keyed
    /// by task name. Surfaced via /health/full so operators can see which
    /// loops are actually running instead of inferring from log archaeology
    /// (full-review AM5).
    maintenance_task_ticks: Mutex<std::collections::BTreeMap<&'static str, String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeObservabilitySnapshot {
    pub syslog_udp_listener_state: &'static str,
    pub syslog_tcp_listener_state: &'static str,
    pub syslog_udp_packets_received: u64,
    pub syslog_udp_bytes_received: u64,
    pub syslog_tcp_connections_accepted: u64,
    pub syslog_tcp_connections_active: u64,
    pub syslog_tcp_connections_closed: u64,
    pub syslog_tcp_connections_rejected: u64,
    pub syslog_tcp_lines_received: u64,
    pub syslog_tcp_bytes_received: u64,
    pub syslog_tcp_lines_dropped_oversize: u64,
    pub syslog_write_channel_full_transitions: u64,
    pub syslog_udp_packets_dropped_queue_full: u64,
    pub syslog_tcp_lines_dropped_queue_full: u64,
    pub docker_ingest_events_received: u64,
    pub docker_ingest_log_entries_received: u64,
    pub docker_ingest_parse_errors: u64,
    pub docker_ingest_stream_reconnects: u64,
    pub docker_ingest_stream_failures: u64,
    pub docker_ingest_tasks_spawned: u64,
    pub docker_ingest_host_streams_active: u64,
    pub docker_ingest_container_streams_active: u64,
    pub remote_docker_event_stream_failures: u64,
    pub ingest_entries_enqueued: u64,
    pub ingest_enqueue_errors: u64,
    pub ingest_queue_depth: usize,
    pub ingest_queue_capacity: usize,
    pub ingest_queue_utilization_pct: String,
    pub writer_batches_flushed: u64,
    pub writer_logs_written: u64,
    pub writer_flush_failures: u64,
    pub writer_logs_retained: u64,
    pub writer_logs_discarded: u64,
    pub writer_storage_blocked: bool,
    pub last_ingest_at: Option<String>,
    pub last_write_at: Option<String>,
    pub last_error_at: Option<String>,
    pub last_docker_ingest_event_at: Option<String>,
    pub last_docker_ingest_log_at: Option<String>,
    pub last_docker_ingest_error_at: Option<String>,
    pub last_remote_docker_event_stream_error_at: Option<String>,
    pub last_remote_docker_event_stream_error: Option<String>,
    /// Background task name → last tick timestamp (RFC3339).
    pub maintenance_task_ticks: std::collections::BTreeMap<&'static str, String>,
}

impl RuntimeObservability {
    pub fn set_udp_listener_state(&self, state: ListenerState) {
        self.syslog_udp_listener_state
            .store(state as u8, Ordering::Relaxed);
    }

    pub fn set_tcp_listener_state(&self, state: ListenerState) {
        self.syslog_tcp_listener_state
            .store(state as u8, Ordering::Relaxed);
    }

    pub fn udp_listener_state(&self) -> ListenerState {
        ListenerState::from_u8(self.syslog_udp_listener_state.load(Ordering::Relaxed))
    }

    pub fn tcp_listener_state(&self) -> ListenerState {
        ListenerState::from_u8(self.syslog_tcp_listener_state.load(Ordering::Relaxed))
    }

    /// True when any syslog listener that was started has since died. Used by
    /// the /health probe so a dead listener turns the container unhealthy and
    /// Docker's restart policy can recover it. `NotStarted` (stdio/query-only
    /// mode, tests) never counts as down.
    pub fn any_listener_down(&self) -> bool {
        self.udp_listener_state() == ListenerState::Down
            || self.tcp_listener_state() == ListenerState::Down
    }

    pub fn set_queue_capacity(&self, capacity: usize) {
        self.ingest_queue_capacity
            .store(capacity, Ordering::Relaxed);
    }

    pub fn set_queue_depth(&self, depth: usize) {
        self.ingest_queue_depth.store(depth, Ordering::Relaxed);
    }

    pub fn record_udp_packet(&self, bytes: usize) {
        self.syslog_udp_packets_received
            .fetch_add(1, Ordering::Relaxed);
        self.syslog_udp_bytes_received
            .fetch_add(bytes as u64, Ordering::Relaxed);
        self.touch_ingest();
    }

    pub fn record_tcp_connection_accepted(&self) {
        self.syslog_tcp_connections_accepted
            .fetch_add(1, Ordering::Relaxed);
        self.syslog_tcp_connections_active
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_tcp_connection_closed(&self) {
        self.syslog_tcp_connections_closed
            .fetch_add(1, Ordering::Relaxed);
        self.syslog_tcp_connections_active
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_sub(1))
            })
            .ok();
    }

    pub fn record_tcp_connection_rejected(&self) {
        self.syslog_tcp_connections_rejected
            .fetch_add(1, Ordering::Relaxed);
        self.touch_error();
    }

    pub fn record_tcp_line(&self, bytes: usize) {
        self.syslog_tcp_lines_received
            .fetch_add(1, Ordering::Relaxed);
        self.syslog_tcp_bytes_received
            .fetch_add(bytes as u64, Ordering::Relaxed);
        self.touch_ingest();
    }

    pub fn record_tcp_line_dropped_oversize(&self) {
        self.syslog_tcp_lines_dropped_oversize
            .fetch_add(1, Ordering::Relaxed);
        self.touch_error();
    }

    pub fn record_write_channel_full_transition(&self) {
        self.syslog_write_channel_full_transitions
            .fetch_add(1, Ordering::Relaxed);
        self.touch_error();
    }

    pub fn record_udp_packet_dropped_queue_full(&self, queue_depth: usize) {
        self.syslog_udp_packets_dropped_queue_full
            .fetch_add(1, Ordering::Relaxed);
        self.set_queue_depth(queue_depth);
        self.touch_error();
    }

    pub fn record_tcp_line_dropped_queue_full(&self, queue_depth: usize) {
        self.syslog_tcp_lines_dropped_queue_full
            .fetch_add(1, Ordering::Relaxed);
        self.set_queue_depth(queue_depth);
        self.touch_error();
    }

    pub fn record_docker_ingest_event(&self) {
        self.docker_ingest_events_received
            .fetch_add(1, Ordering::Relaxed);
        *self
            .last_docker_ingest_event_at
            .lock()
            .expect("last_docker_ingest_event_at mutex poisoned") = Some(now_iso());
    }

    pub fn record_docker_ingest_log_entry(&self) {
        self.docker_ingest_log_entries_received
            .fetch_add(1, Ordering::Relaxed);
        *self
            .last_docker_ingest_log_at
            .lock()
            .expect("last_docker_ingest_log_at mutex poisoned") = Some(now_iso());
        self.touch_ingest();
    }

    pub fn record_docker_ingest_parse_error(&self) {
        self.docker_ingest_parse_errors
            .fetch_add(1, Ordering::Relaxed);
        self.touch_docker_ingest_error();
    }

    pub fn record_docker_ingest_stream_reconnect(&self) {
        self.docker_ingest_stream_reconnects
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_docker_ingest_stream_failure(&self) {
        self.docker_ingest_stream_failures
            .fetch_add(1, Ordering::Relaxed);
        self.touch_docker_ingest_error();
    }

    pub fn record_docker_ingest_task_spawned(&self) {
        self.docker_ingest_tasks_spawned
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_docker_ingest_host_stream_started(&self) {
        self.docker_ingest_host_streams_active
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_docker_ingest_host_stream_ended(&self) {
        self.docker_ingest_host_streams_active
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_sub(1))
            })
            .ok();
    }

    pub fn record_docker_ingest_container_stream_started(&self) {
        self.docker_ingest_container_streams_active
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_docker_ingest_container_stream_ended(&self) {
        self.docker_ingest_container_streams_active
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_sub(1))
            })
            .ok();
    }

    pub fn record_remote_docker_event_stream_failure(&self, host: &str, error: &str) {
        self.remote_docker_event_stream_failures
            .fetch_add(1, Ordering::Relaxed);
        let now = now_iso();
        *self
            .last_remote_docker_event_stream_error_at
            .lock()
            .expect("last_remote_docker_event_stream_error_at mutex poisoned") = Some(now);
        *self
            .last_remote_docker_event_stream_error
            .lock()
            .expect("last_remote_docker_event_stream_error mutex poisoned") =
            Some(format!("{host}: {error}"));
        self.touch_error();
    }

    pub fn record_enqueue_ok(&self, queue_depth: usize) {
        self.ingest_entries_enqueued.fetch_add(1, Ordering::Relaxed);
        self.set_queue_depth(queue_depth);
    }

    pub fn record_enqueue_error(&self, queue_depth: usize) {
        self.ingest_enqueue_errors.fetch_add(1, Ordering::Relaxed);
        self.set_queue_depth(queue_depth);
        self.touch_error();
    }

    pub fn record_writer_flushed(&self, logs_written: usize) {
        self.writer_batches_flushed.fetch_add(1, Ordering::Relaxed);
        self.writer_logs_written
            .fetch_add(logs_written as u64, Ordering::Relaxed);
        self.writer_storage_blocked.store(false, Ordering::Relaxed);
        *self
            .last_write_at
            .lock()
            .expect("last_write_at mutex poisoned") = Some(now_iso());
    }

    pub fn record_writer_retained(&self, retained: usize, storage_blocked: bool) {
        self.writer_flush_failures.fetch_add(1, Ordering::Relaxed);
        self.writer_logs_retained
            .fetch_add(retained as u64, Ordering::Relaxed);
        self.writer_storage_blocked
            .store(storage_blocked, Ordering::Relaxed);
        self.touch_error();
    }

    pub fn record_writer_discarded(&self, discarded: usize) {
        self.writer_flush_failures.fetch_add(1, Ordering::Relaxed);
        self.writer_logs_discarded
            .fetch_add(discarded as u64, Ordering::Relaxed);
        self.touch_error();
    }

    pub fn snapshot(&self) -> RuntimeObservabilitySnapshot {
        let queue_depth = self.ingest_queue_depth.load(Ordering::Relaxed);
        let queue_capacity = self.ingest_queue_capacity.load(Ordering::Relaxed);
        let queue_utilization = if queue_capacity == 0 {
            0.0
        } else {
            (queue_depth as f64 / queue_capacity as f64) * 100.0
        };
        RuntimeObservabilitySnapshot {
            syslog_udp_listener_state: self.udp_listener_state().as_str(),
            syslog_tcp_listener_state: self.tcp_listener_state().as_str(),
            syslog_udp_packets_received: self.syslog_udp_packets_received.load(Ordering::Relaxed),
            syslog_udp_bytes_received: self.syslog_udp_bytes_received.load(Ordering::Relaxed),
            syslog_tcp_connections_accepted: self
                .syslog_tcp_connections_accepted
                .load(Ordering::Relaxed),
            syslog_tcp_connections_active: self
                .syslog_tcp_connections_active
                .load(Ordering::Relaxed),
            syslog_tcp_connections_closed: self
                .syslog_tcp_connections_closed
                .load(Ordering::Relaxed),
            syslog_tcp_connections_rejected: self
                .syslog_tcp_connections_rejected
                .load(Ordering::Relaxed),
            syslog_tcp_lines_received: self.syslog_tcp_lines_received.load(Ordering::Relaxed),
            syslog_tcp_bytes_received: self.syslog_tcp_bytes_received.load(Ordering::Relaxed),
            syslog_tcp_lines_dropped_oversize: self
                .syslog_tcp_lines_dropped_oversize
                .load(Ordering::Relaxed),
            syslog_write_channel_full_transitions: self
                .syslog_write_channel_full_transitions
                .load(Ordering::Relaxed),
            syslog_udp_packets_dropped_queue_full: self
                .syslog_udp_packets_dropped_queue_full
                .load(Ordering::Relaxed),
            syslog_tcp_lines_dropped_queue_full: self
                .syslog_tcp_lines_dropped_queue_full
                .load(Ordering::Relaxed),
            docker_ingest_events_received: self
                .docker_ingest_events_received
                .load(Ordering::Relaxed),
            docker_ingest_log_entries_received: self
                .docker_ingest_log_entries_received
                .load(Ordering::Relaxed),
            docker_ingest_parse_errors: self.docker_ingest_parse_errors.load(Ordering::Relaxed),
            docker_ingest_stream_reconnects: self
                .docker_ingest_stream_reconnects
                .load(Ordering::Relaxed),
            docker_ingest_stream_failures: self
                .docker_ingest_stream_failures
                .load(Ordering::Relaxed),
            docker_ingest_tasks_spawned: self.docker_ingest_tasks_spawned.load(Ordering::Relaxed),
            docker_ingest_host_streams_active: self
                .docker_ingest_host_streams_active
                .load(Ordering::Relaxed),
            docker_ingest_container_streams_active: self
                .docker_ingest_container_streams_active
                .load(Ordering::Relaxed),
            remote_docker_event_stream_failures: self
                .remote_docker_event_stream_failures
                .load(Ordering::Relaxed),
            ingest_entries_enqueued: self.ingest_entries_enqueued.load(Ordering::Relaxed),
            ingest_enqueue_errors: self.ingest_enqueue_errors.load(Ordering::Relaxed),
            ingest_queue_depth: queue_depth,
            ingest_queue_capacity: queue_capacity,
            ingest_queue_utilization_pct: format!("{queue_utilization:.2}"),
            writer_batches_flushed: self.writer_batches_flushed.load(Ordering::Relaxed),
            writer_logs_written: self.writer_logs_written.load(Ordering::Relaxed),
            writer_flush_failures: self.writer_flush_failures.load(Ordering::Relaxed),
            writer_logs_retained: self.writer_logs_retained.load(Ordering::Relaxed),
            writer_logs_discarded: self.writer_logs_discarded.load(Ordering::Relaxed),
            writer_storage_blocked: self.writer_storage_blocked.load(Ordering::Relaxed),
            last_ingest_at: self
                .last_ingest_at
                .lock()
                .expect("last_ingest_at mutex poisoned")
                .clone(),
            last_write_at: self
                .last_write_at
                .lock()
                .expect("last_write_at mutex poisoned")
                .clone(),
            last_error_at: self
                .last_error_at
                .lock()
                .expect("last_error_at mutex poisoned")
                .clone(),
            last_docker_ingest_event_at: self
                .last_docker_ingest_event_at
                .lock()
                .expect("last_docker_ingest_event_at mutex poisoned")
                .clone(),
            last_docker_ingest_log_at: self
                .last_docker_ingest_log_at
                .lock()
                .expect("last_docker_ingest_log_at mutex poisoned")
                .clone(),
            last_docker_ingest_error_at: self
                .last_docker_ingest_error_at
                .lock()
                .expect("last_docker_ingest_error_at mutex poisoned")
                .clone(),
            last_remote_docker_event_stream_error_at: self
                .last_remote_docker_event_stream_error_at
                .lock()
                .expect("last_remote_docker_event_stream_error_at mutex poisoned")
                .clone(),
            last_remote_docker_event_stream_error: self
                .last_remote_docker_event_stream_error
                .lock()
                .expect("last_remote_docker_event_stream_error mutex poisoned")
                .clone(),
            maintenance_task_ticks: self
                .maintenance_task_ticks
                .lock()
                .expect("maintenance_task_ticks mutex poisoned")
                .clone(),
        }
    }

    /// Record that a named background task completed a loop iteration.
    pub fn record_task_tick(&self, task: &'static str) {
        self.maintenance_task_ticks
            .lock()
            .expect("maintenance_task_ticks mutex poisoned")
            .insert(task, now_iso());
    }

    fn touch_ingest(&self) {
        *self
            .last_ingest_at
            .lock()
            .expect("last_ingest_at mutex poisoned") = Some(now_iso());
    }

    fn touch_error(&self) {
        *self
            .last_error_at
            .lock()
            .expect("last_error_at mutex poisoned") = Some(now_iso());
    }

    fn touch_docker_ingest_error(&self) {
        *self
            .last_docker_ingest_error_at
            .lock()
            .expect("last_docker_ingest_error_at mutex poisoned") = Some(now_iso());
        self.touch_error();
    }
}

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reports_queue_utilization() {
        let obs = RuntimeObservability::default();
        obs.set_queue_capacity(100);
        obs.set_queue_depth(25);

        let snapshot = obs.snapshot();

        assert_eq!(snapshot.ingest_queue_depth, 25);
        assert_eq!(snapshot.ingest_queue_capacity, 100);
        assert_eq!(snapshot.ingest_queue_utilization_pct, "25.00");
    }

    #[test]
    fn tcp_connection_active_count_saturates() {
        let obs = RuntimeObservability::default();
        obs.record_tcp_connection_closed();
        assert_eq!(obs.snapshot().syslog_tcp_connections_active, 0);

        obs.record_tcp_connection_accepted();
        obs.record_tcp_connection_closed();
        assert_eq!(obs.snapshot().syslog_tcp_connections_active, 0);
    }

    #[test]
    fn snapshot_reports_docker_ingest_counters() {
        let obs = RuntimeObservability::default();
        obs.record_docker_ingest_event();
        obs.record_docker_ingest_log_entry();
        obs.record_docker_ingest_parse_error();
        obs.record_docker_ingest_stream_reconnect();
        obs.record_docker_ingest_stream_failure();
        obs.record_docker_ingest_task_spawned();
        obs.record_docker_ingest_host_stream_started();
        obs.record_docker_ingest_container_stream_started();
        obs.record_remote_docker_event_stream_failure("tootie", "exit status 255");

        let snapshot = obs.snapshot();

        assert_eq!(snapshot.docker_ingest_events_received, 1);
        assert_eq!(snapshot.docker_ingest_log_entries_received, 1);
        assert_eq!(snapshot.docker_ingest_parse_errors, 1);
        assert_eq!(snapshot.docker_ingest_stream_reconnects, 1);
        assert_eq!(snapshot.docker_ingest_stream_failures, 1);
        assert_eq!(snapshot.docker_ingest_tasks_spawned, 1);
        assert_eq!(snapshot.docker_ingest_host_streams_active, 1);
        assert_eq!(snapshot.docker_ingest_container_streams_active, 1);
        assert_eq!(snapshot.remote_docker_event_stream_failures, 1);
        assert!(snapshot.last_ingest_at.is_some());
        assert!(snapshot.last_error_at.is_some());
        assert!(snapshot.last_docker_ingest_event_at.is_some());
        assert!(snapshot.last_docker_ingest_log_at.is_some());
        assert!(snapshot.last_docker_ingest_error_at.is_some());
        assert!(snapshot.last_remote_docker_event_stream_error_at.is_some());
        assert_eq!(
            snapshot.last_remote_docker_event_stream_error.as_deref(),
            Some("tootie: exit status 255")
        );

        obs.record_docker_ingest_host_stream_ended();
        obs.record_docker_ingest_host_stream_ended();
        obs.record_docker_ingest_container_stream_ended();
        obs.record_docker_ingest_container_stream_ended();

        let snapshot = obs.snapshot();
        assert_eq!(snapshot.docker_ingest_host_streams_active, 0);
        assert_eq!(snapshot.docker_ingest_container_streams_active, 0);
    }

    #[test]
    fn snapshot_reports_queue_pressure_counters() {
        let obs = RuntimeObservability::default();
        obs.record_write_channel_full_transition();
        obs.record_udp_packet_dropped_queue_full(10);
        obs.record_tcp_line_dropped_queue_full(20);

        let snapshot = obs.snapshot();

        assert_eq!(snapshot.syslog_write_channel_full_transitions, 1);
        assert_eq!(snapshot.syslog_udp_packets_dropped_queue_full, 1);
        assert_eq!(snapshot.syslog_tcp_lines_dropped_queue_full, 1);
        assert_eq!(snapshot.ingest_queue_depth, 20);
        assert!(snapshot.last_error_at.is_some());
    }
}
