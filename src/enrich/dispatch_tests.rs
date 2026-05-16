use crate::db::LogBatchEntry;
use crate::enrich::EnrichmentPipeline;

fn fixture_entry() -> LogBatchEntry {
    LogBatchEntry {
        timestamp: "2026-05-16T10:00:00Z".into(),
        hostname: "h".into(),
        facility: None,
        severity: "info".into(),
        app_name: Some("unknown-app".into()),
        process_id: None,
        message: "hello".into(),
        raw: "hello".into(),
        source_ip: "udp://127.0.0.1:5678".into(),
        docker_checkpoint: None,
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
        metadata_json: None,
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

#[test]
fn empty_pipeline_leaves_entry_unchanged() {
    let pipeline = EnrichmentPipeline::new();
    let mut entry = fixture_entry();
    pipeline.dispatch(&mut entry);
    assert!(entry.http_status.is_none());
    assert!(entry.event_action.is_none());
    assert!(entry.parse_error.is_none());
    assert!(entry.metadata_json.is_none());
}

use crate::enrich::SourceKind;

fn entry_with_source(
    app_name: Option<&str>,
    container_name: Option<&str>,
    message: &str,
    source_kind: SourceKind,
) -> LogBatchEntry {
    let metadata = if let Some(c) = container_name {
        format!(
            r#"{{"source_kind":"{}","docker":{{"container_name":"{}"}}}}"#,
            source_kind.as_str(),
            c
        )
    } else {
        format!(r#"{{"source_kind":"{}"}}"#, source_kind.as_str())
    };
    LogBatchEntry {
        timestamp: "2026-05-16T10:00:00Z".into(),
        hostname: "h".into(),
        facility: None,
        severity: "info".into(),
        app_name: app_name.map(str::to_string),
        process_id: None,
        message: message.into(),
        raw: message.into(),
        source_ip: "udp://127.0.0.1:5678".into(),
        docker_checkpoint: None,
        ai_tool: None,
        ai_project: None,
        ai_session_id: None,
        ai_transcript_path: None,
        metadata_json: Some(metadata),
        http_status: None,
        auth_outcome: None,
        dns_blocked: None,
        event_action: None,
        parse_error: None,
    }
}

#[test]
fn dispatch_swag_container_to_swag_parser() {
    let pipeline = EnrichmentPipeline::new();
    let mut entry = entry_with_source(
        Some("nginx"),
        Some("swag"),
        r#"192.0.2.55 - - [15/May/2026:14:22:11 +0000] "GET / HTTP/1.1" 200 100 "-" "ua""#,
        SourceKind::DockerStream,
    );
    pipeline.dispatch(&mut entry);
    assert_eq!(entry.http_status, Some(200));
    assert_eq!(entry.event_action.as_deref(), Some("http_request"));
}

#[test]
fn dispatch_docker_event_by_source_kind() {
    let pipeline = EnrichmentPipeline::new();
    let mut entry = entry_with_source(
        Some("dockerd"),
        None,
        "docker container event: die container=postgres image=postgres:16 compose_project=stack compose_service=db exit_code=137",
        SourceKind::DockerEvent,
    );
    pipeline.dispatch(&mut entry);
    assert_eq!(entry.event_action.as_deref(), Some("die"));
}

#[test]
fn dispatch_authelia_main_to_authelia_parser() {
    let pipeline = EnrichmentPipeline::new();
    let mut entry = entry_with_source(
        Some("authelia"),
        Some("authelia-main"),
        r#"{"level":"info","msg":"Authentication attempt successful","path":"/api/firstfactor","remote_ip":"100.0.0.1","time":"2026-05-15T03:46:03Z","username":"alice"}"#,
        SourceKind::DockerStream,
    );
    pipeline.dispatch(&mut entry);
    assert_eq!(entry.auth_outcome, Some("success"));
}

#[test]
fn dispatch_adguard_api_to_adguard_parser() {
    let pipeline = EnrichmentPipeline::new();
    let mut entry = entry_with_source(
        Some("adguard-query"),
        None,
        r#"{"T":"2026-05-15T14:22:11.123Z","QH":"doubleclick.net","QT":"A","Client":"192.168.10.55","Result":{"IsFiltered":true,"Reason":"FilteredBlackList"}}"#,
        SourceKind::AdguardApi,
    );
    pipeline.dispatch(&mut entry);
    assert_eq!(entry.dns_blocked, Some(true));
}

#[test]
fn dispatch_unknown_source_no_op() {
    let pipeline = EnrichmentPipeline::new();
    let mut entry = entry_with_source(
        Some("randomapp"),
        None,
        "hello world",
        SourceKind::SyslogTcp,
    );
    pipeline.dispatch(&mut entry);
    assert!(entry.event_action.is_none());
    assert!(entry.parse_error.is_none());
}

#[test]
fn dispatch_records_parse_error_on_parser_failure() {
    let pipeline = EnrichmentPipeline::new();
    let mut entry = entry_with_source(
        Some("adguard-query"),
        None,
        "{ bad json",
        SourceKind::AdguardApi,
    );
    pipeline.dispatch(&mut entry);
    assert!(entry.parse_error.is_some());
    assert!(entry.parse_error.as_ref().unwrap().starts_with("adguard:"));
}

#[test]
fn dispatch_kernel_via_app_name() {
    let pipeline = EnrichmentPipeline::new();
    let mut entry = entry_with_source(
        Some("kernel"),
        None,
        "Out of memory: Killed process 100 (foo) total-vm:1024kB, anon-rss:512kB, file-rss:0kB, shmem-rss:0kB, UID:0 pgtables:8kB oom_score_adj:0",
        SourceKind::SyslogTcp,
    );
    pipeline.dispatch(&mut entry);
    assert_eq!(entry.event_action.as_deref(), Some("oom_kill"));
}
