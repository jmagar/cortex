use tokio::io::AsyncWriteExt;

use crate::db::LogBatchEntry;
use crate::ingest::IngestTx;

use super::models::FileTailSource;
use super::registry::FileTailRegistry;
use super::supervisor::{FileTailSupervisor, file_tail_line_to_entry, tail_file_once_for_test};

fn source(id: &str, path: &str, tag: &str) -> FileTailSource {
    FileTailSource {
        id: id.into(),
        path: path.into(),
        tag: tag.into(),
        hostname: Some("squirts".into()),
        facility: Some("local4".into()),
        severity: "info".into(),
        start_at_end: true,
        enabled: true,
        created_at: "2026-06-11T20:00:00Z".into(),
        updated_at: "2026-06-11T20:00:00Z".into(),
    }
}

#[test]
fn file_tail_line_to_entry_sets_expected_envelope() {
    let source = source("swag-access", "/tmp/access.log", "swag-access");

    let entry = file_tail_line_to_entry(&source, "GET / HTTP/1.1\" 401", "2026-06-11T20:01:00Z");

    assert_eq!(entry.timestamp, "2026-06-11T20:01:00Z");
    assert_eq!(entry.hostname, "squirts");
    assert_eq!(entry.facility.as_deref(), Some("local4"));
    assert_eq!(entry.severity, "info");
    assert_eq!(entry.app_name.as_deref(), Some("swag-access"));
    assert_eq!(entry.message, "GET / HTTP/1.1\" 401");
    assert_eq!(entry.raw, "GET / HTTP/1.1\" 401");
    assert_eq!(entry.source_ip, "file-tail://squirts/swag-access");
    assert!(
        entry
            .metadata_json
            .as_deref()
            .unwrap()
            .contains("\"source_kind\":\"file-tail\"")
    );
    assert!(
        entry
            .metadata_json
            .as_deref()
            .unwrap()
            .contains("\"path\":\"/tmp/access.log\"")
    );
}

#[tokio::test]
async fn reconcile_restarts_task_when_source_definition_changes() {
    let temp = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(FileTailRegistry::new(temp.path().join("file-tails.json")));
    let (tx, _rx) = tokio::sync::mpsc::channel::<LogBatchEntry>(4);
    let ingest = IngestTx::from_sender_for_test(tx);
    let supervisor = FileTailSupervisor::new(
        std::sync::Arc::clone(&registry),
        ingest,
        tokio_util::sync::CancellationToken::new(),
    );

    registry
        .upsert(source("swag-access", "/tmp/one.log", "swag-access"))
        .unwrap();
    supervisor.reconcile().unwrap();
    assert_eq!(
        supervisor
            .running_source_for_test("swag-access")
            .unwrap()
            .path,
        "/tmp/one.log"
    );

    registry
        .upsert(source("swag-access", "/tmp/two.log", "swag-access"))
        .unwrap();
    supervisor.reconcile().unwrap();
    assert_eq!(
        supervisor
            .running_source_for_test("swag-access")
            .unwrap()
            .path,
        "/tmp/two.log"
    );
    supervisor.shutdown();
}

#[tokio::test]
async fn tail_file_once_sends_existing_lines_when_not_starting_at_end() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("authelia.log");
    let mut file = tokio::fs::File::create(&file_path).await.unwrap();
    file.write_all(b"time=one level=info\n").await.unwrap();
    file.write_all(b"time=two level=error\n").await.unwrap();
    file.flush().await.unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<LogBatchEntry>(4);
    let ingest = IngestTx::from_sender_for_test(tx);
    let source = FileTailSource {
        id: "authelia".into(),
        path: file_path.to_string_lossy().into_owned(),
        tag: "authelia".into(),
        hostname: Some("squirts".into()),
        facility: Some("local5".into()),
        severity: "info".into(),
        start_at_end: false,
        enabled: true,
        created_at: "2026-06-11T20:00:00Z".into(),
        updated_at: "2026-06-11T20:00:00Z".into(),
    };

    tail_file_once_for_test(source, ingest).await.unwrap();

    assert_eq!(rx.recv().await.unwrap().message, "time=one level=info");
    assert_eq!(rx.recv().await.unwrap().message, "time=two level=error");
    assert!(rx.try_recv().is_err());
}
