//! Tests for the OTLP HTTP receiver (logs handler, severity mapping,
//! AnyValue extraction, auth gate, status-code contract).

use super::*;

use std::sync::atomic::Ordering;

use opentelemetry_proto::tonic::{
    collector::logs::v1::ExportLogsServiceRequest,
    common::v1::{any_value::Value as AnyValueKind, AnyValue, KeyValue},
    logs::v1::{LogRecord, ResourceLogs, ScopeLogs},
    resource::v1::Resource,
};

fn av_string(s: &str) -> AnyValue {
    AnyValue {
        value: Some(AnyValueKind::StringValue(s.to_string())),
    }
}

fn kv(key: &str, value: AnyValue) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value),
    }
}

fn sample_request(
    host: &str,
    service: &str,
    body: &str,
    severity: i32,
) -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: vec![
                    kv("host.name", av_string(host)),
                    kv("service.name", av_string(service)),
                ],
                dropped_attributes_count: 0,
                entity_refs: vec![],
            }),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![LogRecord {
                    time_unix_nano: 1_700_000_000_000_000_000,
                    observed_time_unix_nano: 0,
                    severity_number: severity,
                    severity_text: String::new(),
                    body: Some(av_string(body)),
                    attributes: vec![],
                    dropped_attributes_count: 0,
                    flags: 0,
                    trace_id: vec![],
                    span_id: vec![],
                    event_name: String::new(),
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    }
}

// ---- severity mapping ----

#[test]
fn severity_unspecified_falls_back_to_info() {
    assert_eq!(severity_from_number(0), "info");
    assert_eq!(severity_from_number(99), "info");
    assert_eq!(severity_from_number(-1), "info");
}

#[test]
fn severity_buckets_match_otlp_spec() {
    assert_eq!(severity_from_number(1), "debug"); // TRACE
    assert_eq!(severity_from_number(5), "debug"); // DEBUG
    assert_eq!(severity_from_number(9), "info"); // INFO
    assert_eq!(severity_from_number(13), "warning"); // WARN
    assert_eq!(severity_from_number(17), "err"); // ERROR
    assert_eq!(severity_from_number(21), "crit"); // FATAL
}

// ---- AnyValue extraction ----

#[test]
fn any_value_string() {
    assert_eq!(any_value_to_string(&av_string("hi")).as_deref(), Some("hi"));
}

#[test]
fn any_value_none_returns_none() {
    let v = AnyValue { value: None };
    assert!(any_value_to_string(&v).is_none());
}

#[test]
fn any_value_int_and_bool_stringify() {
    let int_val = AnyValue {
        value: Some(AnyValueKind::IntValue(42)),
    };
    assert_eq!(any_value_to_string(&int_val).as_deref(), Some("42"));
    let bool_val = AnyValue {
        value: Some(AnyValueKind::BoolValue(true)),
    };
    assert_eq!(any_value_to_string(&bool_val).as_deref(), Some("true"));
}

// ---- build_entries ----

#[test]
fn build_entries_extracts_resource_attrs() {
    let peer = "127.0.0.1:12345".parse().unwrap();
    let req = sample_request("dookie", "claude-code", "tool_call started", 9);
    let entries = build_entries(&req, peer);
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.hostname, "dookie");
    assert_eq!(e.app_name.as_deref(), Some("claude-code"));
    assert_eq!(e.message, "tool_call started");
    assert_eq!(e.severity, "info");
    assert_eq!(e.facility.as_deref(), Some("otlp"));
    assert_eq!(e.source_ip, "127.0.0.1:12345");
    let metadata: serde_json::Value =
        serde_json::from_str(e.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["source_type"], "otlp");
    assert_eq!(metadata["peer_ip"], "127.0.0.1");
    assert_eq!(metadata["peer_port"], 12345);
    assert_eq!(metadata["service_name"], "claude-code");
    assert_eq!(metadata["resource_attributes"]["host.name"], "dookie");
}

#[test]
fn build_entries_missing_host_name_uses_empty_string() {
    let peer = "127.0.0.1:1".parse().unwrap();
    let req = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: vec![],
                dropped_attributes_count: 0,
                entity_refs: vec![],
            }),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![LogRecord {
                    time_unix_nano: 0,
                    observed_time_unix_nano: 0,
                    severity_number: 9,
                    severity_text: String::new(),
                    body: Some(av_string("orphan")),
                    attributes: vec![],
                    dropped_attributes_count: 0,
                    flags: 0,
                    trace_id: vec![],
                    span_id: vec![],
                    event_name: String::new(),
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };
    let entries = build_entries(&req, peer);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].hostname, "");
    assert!(entries[0].app_name.is_none());
}

#[test]
fn build_entries_no_panic_on_empty_body() {
    let peer = "127.0.0.1:1".parse().unwrap();
    let req = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: None,
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![LogRecord {
                    time_unix_nano: 0,
                    observed_time_unix_nano: 0,
                    severity_number: 9,
                    severity_text: String::new(),
                    body: None,
                    attributes: vec![],
                    dropped_attributes_count: 0,
                    flags: 0,
                    trace_id: vec![],
                    span_id: vec![],
                    event_name: String::new(),
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };
    let entries = build_entries(&req, peer);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].message, "");
}

#[test]
fn build_entries_handles_multiple_resource_logs() {
    let peer = "10.0.0.1:9999".parse().unwrap();
    let req = ExportLogsServiceRequest {
        resource_logs: vec![
            sample_request("dookie", "claude-code", "msg one", 9)
                .resource_logs
                .into_iter()
                .next()
                .unwrap(),
            sample_request("squirts", "codex", "msg two", 17)
                .resource_logs
                .into_iter()
                .next()
                .unwrap(),
        ],
    };
    let entries = build_entries(&req, peer);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].hostname, "dookie");
    assert_eq!(entries[1].hostname, "squirts");
    assert_eq!(entries[1].severity, "err");
}

#[test]
fn build_entries_extracts_ai_metadata_from_attributes() {
    let peer = "127.0.0.1:1".parse().unwrap();
    let req = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: vec![
                    kv("host.name", av_string("tootie")),
                    kv("service.name", av_string("claude-code")),
                    kv("session.id", av_string("res-session-123")),
                ],
                dropped_attributes_count: 0,
                entity_refs: vec![],
            }),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![LogRecord {
                    time_unix_nano: 0,
                    observed_time_unix_nano: 0,
                    severity_number: 9,
                    severity_text: String::new(),
                    body: Some(av_string("msg with log-level attributes")),
                    attributes: vec![
                        kv("session.id", av_string("log-session-456")), // overrides resource
                        kv("project.path", av_string("/work/syslog-mcp")),
                        kv("Authorization", av_string("Bearer secret")),
                    ],
                    dropped_attributes_count: 0,
                    flags: 0,
                    trace_id: vec![],
                    span_id: vec![],
                    event_name: String::new(),
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };

    let entries = build_entries(&req, peer);
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.hostname, "tootie");
    assert_eq!(e.app_name.as_deref(), Some("claude-code"));
    assert_eq!(e.ai_tool, None);
    assert_eq!(e.ai_session_id.as_deref(), Some("log-session-456"));
    assert_eq!(e.ai_project.as_deref(), Some("/work/syslog-mcp"));

    let metadata: serde_json::Value =
        serde_json::from_str(e.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["log_attributes"]["Authorization"], "[REDACTED]");
}

#[test]
fn build_entries_extracts_ai_tool_from_explicit_attribute() {
    let peer = "127.0.0.1:1".parse().unwrap();
    let req = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: vec![kv("host.name", av_string("tootie"))],
                dropped_attributes_count: 0,
                entity_refs: vec![],
            }),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![LogRecord {
                    time_unix_nano: 0,
                    observed_time_unix_nano: 0,
                    severity_number: 9,
                    severity_text: String::new(),
                    body: Some(av_string("msg")),
                    attributes: vec![kv("ai.tool", av_string("claude"))],
                    dropped_attributes_count: 0,
                    flags: 0,
                    trace_id: vec![],
                    span_id: vec![],
                    event_name: String::new(),
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };

    let entries = build_entries(&req, peer);
    assert_eq!(entries[0].ai_tool.as_deref(), Some("claude"));
}

#[test]
fn build_entries_ignores_unknown_or_oversized_ai_tool() {
    let peer = "127.0.0.1:1".parse().unwrap();
    let req = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: vec![kv("host.name", av_string("tootie"))],
                dropped_attributes_count: 0,
                entity_refs: vec![],
            }),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![
                    LogRecord {
                        time_unix_nano: 0,
                        observed_time_unix_nano: 0,
                        severity_number: 9,
                        severity_text: String::new(),
                        body: Some(av_string("msg")),
                        attributes: vec![kv("ai.tool", av_string("unknown"))],
                        dropped_attributes_count: 0,
                        flags: 0,
                        trace_id: vec![],
                        span_id: vec![],
                        event_name: String::new(),
                    },
                    LogRecord {
                        time_unix_nano: 0,
                        observed_time_unix_nano: 0,
                        severity_number: 9,
                        severity_text: String::new(),
                        body: Some(av_string("msg")),
                        attributes: vec![kv("ai.tool", av_string(&"x".repeat(65)))],
                        dropped_attributes_count: 0,
                        flags: 0,
                        trace_id: vec![],
                        span_id: vec![],
                        event_name: String::new(),
                    },
                ],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };

    let entries = build_entries(&req, peer);
    assert_eq!(entries[0].ai_tool, None);
    assert_eq!(entries[1].ai_tool, None);
}

#[test]
fn build_entries_ignores_oversized_ai_project_and_session_id() {
    let peer = "127.0.0.1:1".parse().unwrap();
    let req = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: vec![
                    kv("host.name", av_string("tootie")),
                    kv("project.path", av_string(&"p".repeat(513))),
                    kv("session.id", av_string(&"s".repeat(129))),
                ],
                dropped_attributes_count: 0,
                entity_refs: vec![],
            }),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![LogRecord {
                    time_unix_nano: 0,
                    observed_time_unix_nano: 0,
                    severity_number: 9,
                    severity_text: String::new(),
                    body: Some(av_string("msg")),
                    attributes: vec![],
                    dropped_attributes_count: 0,
                    flags: 0,
                    trace_id: vec![],
                    span_id: vec![],
                    event_name: String::new(),
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };

    let entries = build_entries(&req, peer);
    assert_eq!(entries[0].ai_project, None);
    assert_eq!(entries[0].ai_session_id, None);
}

// ---- auth gate ----

fn state_with_token(token: Option<&str>) -> OtlpState {
    let (tx, _rx) = tokio::sync::mpsc::channel::<crate::db::LogBatchEntry>(10);
    let ingest = crate::ingest::IngestTx::from_sender_for_test(tx);
    OtlpState::new(
        ingest,
        token.map(String::from),
        Arc::new(OtlpCounters::default()),
    )
}

#[test]
fn auth_disabled_when_no_token() {
    let state = state_with_token(None);
    let headers = HeaderMap::new();
    assert!(is_authorized(&state, &headers));
}

#[test]
fn auth_required_with_correct_bearer() {
    let state = state_with_token(Some("secret"));
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        HeaderValue::from_static("Bearer secret"),
    );
    assert!(is_authorized(&state, &headers));
}

#[test]
fn auth_rejects_wrong_token() {
    let state = state_with_token(Some("secret"));
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        HeaderValue::from_static("Bearer wrong"),
    );
    assert!(!is_authorized(&state, &headers));
}

#[test]
fn auth_rejects_missing_header() {
    let state = state_with_token(Some("secret"));
    let headers = HeaderMap::new();
    assert!(!is_authorized(&state, &headers));
}

#[test]
fn auth_rejects_non_bearer_scheme() {
    let state = state_with_token(Some("secret"));
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        HeaderValue::from_static("Basic secret"),
    );
    assert!(!is_authorized(&state, &headers));
}

#[tokio::test]
async fn metrics_handler_returns_not_supported() {
    let response = metrics_handler(
        State(state_with_token(None)),
        HeaderMap::new(),
        Bytes::from_static(b"metrics"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ---- counters ----

#[test]
fn counters_default_to_zero() {
    let counters = OtlpCounters::default();
    assert_eq!(counters.logs_received.load(Ordering::Relaxed), 0);
    assert_eq!(counters.decode_errors.load(Ordering::Relaxed), 0);
}
