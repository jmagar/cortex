use std::os::unix::fs::MetadataExt;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

use crate::db::LogBatchEntry;
use crate::ingest::IngestTx;

use super::models::FileTailSource;
use super::registry::FileTailRegistry;
use super::supervisor::{
    FileTailSupervisor, file_tail_line_to_entry, open_tail_file, read_bounded_line,
    reopen_if_rotated_or_truncated, tail_file_once_for_test,
};

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
        checkpoint_dev: None,
        checkpoint_ino: None,
        checkpoint_offset: None,
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
            .contains("\"path_basename\":\"access.log\"")
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
        8192,
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
async fn reconcile_does_not_restart_task_for_checkpoint_updates() {
    let temp = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(FileTailRegistry::new(temp.path().join("file-tails.json")));
    let (tx, _rx) = tokio::sync::mpsc::channel::<LogBatchEntry>(4);
    let ingest = IngestTx::from_sender_for_test(tx);
    let supervisor = FileTailSupervisor::new(
        std::sync::Arc::clone(&registry),
        ingest,
        tokio_util::sync::CancellationToken::new(),
        8192,
    );

    registry
        .upsert(source("swag-access", "/tmp/one.log", "swag-access"))
        .unwrap();
    supervisor.reconcile().unwrap();
    registry
        .update_checkpoint("swag-access", 1, 2, 3, "2026-06-11T20:01:00Z")
        .unwrap();
    supervisor.reconcile().unwrap();
    assert_eq!(
        supervisor
            .running_source_for_test("swag-access")
            .unwrap()
            .path,
        "/tmp/one.log"
    );
    supervisor.shutdown();
}

#[tokio::test]
async fn open_tail_file_resumes_matching_checkpoint_before_start_at_end() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("app.log");
    tokio::fs::write(&file_path, b"old\nnew\n").await.unwrap();
    let metadata = std::fs::metadata(&file_path).unwrap();
    let mut src = source("app", &file_path.to_string_lossy(), "app");
    src.start_at_end = true;
    src.checkpoint_dev = Some(metadata.dev());
    src.checkpoint_ino = Some(metadata.ino());
    src.checkpoint_offset = Some(4);

    let mut opened = open_tail_file(&src, true).await.unwrap();
    assert_eq!(opened.position, 4);
    let mut rest = String::new();
    opened.file.read_to_string(&mut rest).await.unwrap();
    assert_eq!(rest, "new\n");
}

#[tokio::test]
async fn reopen_if_rotated_or_truncated_detects_rename_create_rotation() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("app.log");
    tokio::fs::write(&file_path, b"old\n").await.unwrap();
    let src = source("app", &file_path.to_string_lossy(), "app");
    let old = open_tail_file(&src, false).await.unwrap();

    tokio::fs::rename(&file_path, temp.path().join("app.log.1"))
        .await
        .unwrap();
    tokio::fs::write(&file_path, b"new\n").await.unwrap();

    let reopened = reopen_if_rotated_or_truncated(&src, old.identity, old.position)
        .await
        .unwrap()
        .expect("rotation should reopen");
    assert_eq!(reopened.position, 0);
    assert_ne!(reopened.identity, old.identity);
}

#[tokio::test]
async fn reopen_if_rotated_or_truncated_detects_copytruncate() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("app.log");
    tokio::fs::write(&file_path, b"first line\nsecond line\n")
        .await
        .unwrap();
    let src = source("app", &file_path.to_string_lossy(), "app");
    let mut opened = open_tail_file(&src, false).await.unwrap();
    opened.position = 22;
    tokio::fs::write(&file_path, b"new\n").await.unwrap();

    let reopened = reopen_if_rotated_or_truncated(&src, opened.identity, opened.position)
        .await
        .unwrap()
        .expect("truncate should reopen");
    assert_eq!(reopened.position, 0);
}

#[tokio::test]
async fn read_bounded_line_truncates_oversized_records() {
    let cursor = std::io::Cursor::new(b"abcdef\nnext\n".to_vec());
    let mut reader = BufReader::new(cursor);
    let mut out = Vec::new();

    let first = read_bounded_line(&mut reader, &mut out, 3).await.unwrap();
    assert_eq!(first.bytes_read, 7);
    assert!(first.truncated);
    assert_eq!(out, b"abc");

    out.clear();
    let second = read_bounded_line(&mut reader, &mut out, 3).await.unwrap();
    assert_eq!(second.bytes_read, 5);
    assert!(second.truncated);
    assert_eq!(out, b"nex");
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
        checkpoint_dev: None,
        checkpoint_ino: None,
        checkpoint_offset: None,
        created_at: "2026-06-11T20:00:00Z".into(),
        updated_at: "2026-06-11T20:00:00Z".into(),
    };

    tail_file_once_for_test(source, ingest).await.unwrap();

    assert_eq!(rx.recv().await.unwrap().message, "time=one level=info");
    assert_eq!(rx.recv().await.unwrap().message, "time=two level=error");
    assert!(rx.try_recv().is_err());
}
