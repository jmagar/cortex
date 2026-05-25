use super::*;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::util::ServiceExt;

use crate::config::StorageConfig;

fn test_app(token: Option<&str>) -> (Router, Arc<DbPool>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = StorageConfig::for_test(dir.path().join("heartbeat-test.db"));
    let pool = Arc::new(crate::db::init_pool(&storage).unwrap());
    let state = HeartbeatState::new(
        Arc::clone(&pool),
        token.map(str::to_string),
        AuthPolicy::Mounted { auth_state: None },
    );
    let app = router(state).layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 7], 41000))));
    (app, pool, dir)
}

fn heartbeat_payload() -> Value {
    json!({
        "host": {
            "host_id": "host-1",
            "hostname": "tootie",
            "os": "linux",
            "kernel": "6.8.0",
            "architecture": "x86_64",
            "boot_id": "boot-1",
            "timezone": "America/New_York"
        },
        "sample": {
            "sequence": 42,
            "sampled_at": "2026-05-25T01:02:03Z",
            "uptime_secs": 86400,
            "monotonic_ms": 86400000,
            "collection_ms": 37,
            "partial": false,
            "probe_errors": [],
            "skipped_probes": []
        },
        "agent": {
            "version": "0.32.6",
            "mode": "always_on",
            "interval_secs": 30,
            "push_latency_ms": 12,
            "retry_backlog": 0
        },
        "cpu": {
            "load1": 0.1,
            "load5": 0.2,
            "load15": 0.3,
            "usage_pct": 4.5,
            "iowait_pct": 0.1,
            "steal_pct": 0.0,
            "core_count": 8
        },
        "memory": {
            "mem_total_bytes": 1000,
            "mem_available_bytes": 250,
            "swap_total_bytes": 100,
            "swap_used_bytes": 10
        },
        "disks": [{
            "kind": "mount",
            "name": "/",
            "fs_type": "ext4",
            "bytes_total": 1000,
            "bytes_free": 400,
            "bytes_used": 600
        }],
        "network": [{
            "interface": "eth0",
            "rx_bytes_per_sec": 100.0,
            "tx_bytes_per_sec": 200.0,
            "rx_errors_per_sec": 0.0,
            "tx_errors_per_sec": 1.0
        }],
        "processes": {
            "total": 10,
            "running": 1,
            "sleeping": 9,
            "zombies": 0,
            "top": []
        },
        "containers": {
            "runtime": "docker",
            "reachable": true,
            "running": 3,
            "exited": 1,
            "restarting": 0,
            "unhealthy": 1,
            "details": []
        }
    })
}

async fn post_json(
    app: Router,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({}));
    (status, value)
}

#[tokio::test]
async fn valid_heartbeat_is_accepted_and_persisted() {
    let (app, pool, _dir) = test_app(Some("secret"));
    let (status, value) =
        post_json(app, "/v1/heartbeats", Some("secret"), heartbeat_payload()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(value["accepted"], 1);
    assert!(value["heartbeat_id"].as_i64().unwrap() > 0);

    let conn = pool.get().unwrap();
    let row: (String, String, i64) = conn
        .query_row(
            "SELECT host_id, source_ip, sequence FROM host_heartbeats",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row.0, "host-1");
    assert_eq!(row.1, "10.0.0.7:41000");
    assert_eq!(row.2, 42);

    for table in [
        "heartbeat_cpu",
        "heartbeat_memory",
        "heartbeat_disks",
        "heartbeat_network",
        "heartbeat_processes",
        "heartbeat_containers",
    ] {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1, "expected one row in {table}");
    }
}

#[tokio::test]
async fn duplicate_heartbeat_is_idempotent() {
    let (app, pool, _dir) = test_app(Some("secret"));
    let payload = heartbeat_payload();
    let first = post_json(
        app.clone(),
        "/v1/heartbeats",
        Some("secret"),
        payload.clone(),
    )
    .await;
    let second = post_json(app, "/v1/heartbeats", Some("secret"), payload).await;
    assert_eq!(first.0, StatusCode::ACCEPTED);
    assert_eq!(second.0, StatusCode::ACCEPTED);
    assert_eq!(second.1["accepted"], 0);
    assert_eq!(first.1["heartbeat_id"], second.1["heartbeat_id"]);

    let conn = pool.get().unwrap();
    let parent_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM host_heartbeats", [], |row| row.get(0))
        .unwrap();
    let cpu_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM heartbeat_cpu", [], |row| row.get(0))
        .unwrap();
    assert_eq!(parent_count, 1);
    assert_eq!(cpu_count, 1);
}

#[tokio::test]
async fn bearer_auth_is_required_and_query_tokens_are_ignored() {
    let (app, _pool, _dir) = test_app(Some("secret"));
    for (uri, token) in [
        ("/v1/heartbeats", None),
        ("/v1/heartbeats", Some("wrong")),
        ("/v1/heartbeats?token=secret", None),
    ] {
        let (status, value) = post_json(app.clone(), uri, token, heartbeat_payload()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(value["error"], "unauthorized");
    }

    let (status, _) = post_json(app, "/v1/heartbeats", Some("secret"), heartbeat_payload()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn invalid_payloads_return_invalid_payload() {
    let (app, _pool, _dir) = test_app(Some("secret"));
    let mut payload = heartbeat_payload();
    payload["unexpected"] = json!(true);
    let (status, value) = post_json(app, "/v1/heartbeats", Some("secret"), payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"], "invalid_payload");
}

#[tokio::test]
async fn heartbeat_body_limit_is_route_local_256k() {
    let (app, _pool, _dir) = test_app(Some("secret"));

    let mut accepted = heartbeat_payload();
    accepted["gpu"] = json!({"padding": "x".repeat(70 * 1024)});
    let (status, _) = post_json(app.clone(), "/v1/heartbeats", Some("secret"), accepted).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let mut oversized = heartbeat_payload();
    oversized["gpu"] = json!({"padding": "x".repeat(300 * 1024)});
    let (status, value) = post_json(app, "/v1/heartbeats", Some("secret"), oversized).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(value["error"], "payload_too_large");
}
