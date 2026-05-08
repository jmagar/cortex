use super::*;

#[test]
fn update_backpressure_only_reports_state_transitions() {
    let mut backpressure = false;

    assert_eq!(
        update_backpressure(&mut backpressure, true),
        Some(BackpressureTransition::Applied)
    );
    assert!(backpressure);
    assert_eq!(update_backpressure(&mut backpressure, true), None);
    assert_eq!(
        update_backpressure(&mut backpressure, false),
        Some(BackpressureTransition::Cleared)
    );
    assert!(!backpressure);
    assert_eq!(update_backpressure(&mut backpressure, false), None);
}

#[tokio::test]
async fn tcp_connection_allows_multiple_lines_beyond_connection_total_size() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::db::LogBatchEntry>(16);
    let ingest = crate::ingest::IngestTx::from_sender_for_test(tx);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let accept_task = tokio::spawn(async move {
        let (server_stream, peer) = listener.accept().await.unwrap();
        handle_tcp_connection(server_stream, peer, ingest, 64, 5).await;
    });

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    use tokio::io::AsyncWriteExt;
    client
        .write_all(
            b"<34>Oct 11 22:14:15 host app: first message\n<34>Oct 11 22:14:16 host app: second message\n",
        )
        .await
        .unwrap();
    client.shutdown().await.unwrap();

    let first = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let second = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert!(first.message.contains("first message"));
    assert!(second.message.contains("second message"));

    accept_task.await.unwrap();
}

#[tokio::test]
async fn tcp_connection_closes_oversized_unterminated_line_without_enqueueing() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::db::LogBatchEntry>(16);
    let ingest = crate::ingest::IngestTx::from_sender_for_test(tx);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let accept_task = tokio::spawn(async move {
        let (server_stream, peer) = listener.accept().await.unwrap();
        handle_tcp_connection(server_stream, peer, ingest, 32, 5).await;
    });

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    client.write_all(&vec![b'x'; 128]).await.unwrap();

    let mut buf = [0u8; 1];
    let read = tokio::time::timeout(std::time::Duration::from_secs(1), client.read(&mut buf))
        .await
        .expect("server should close oversized TCP connection")
        .unwrap();
    assert_eq!(read, 0);

    match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
        Ok(Some(entry)) => panic!(
            "oversized unterminated line must not enqueue an entry, got: {:?}",
            entry
        ),
        Ok(None) | Err(_) => {}
    }

    accept_task.await.unwrap();
}

#[tokio::test]
async fn tcp_connection_drops_oversized_delimited_line_and_keeps_later_frames() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::db::LogBatchEntry>(16);
    let ingest = crate::ingest::IngestTx::from_sender_for_test(tx);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let accept_task = tokio::spawn(async move {
        let (server_stream, peer) = listener.accept().await.unwrap();
        handle_tcp_connection(server_stream, peer, ingest, 32, 5).await;
    });

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    use tokio::io::AsyncWriteExt;
    client.write_all(&vec![b'x'; 64]).await.unwrap();
    client.write_all(b"\nvalid\n").await.unwrap();
    client.shutdown().await.unwrap();

    let entry = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(entry.raw.contains("valid"));

    accept_task.await.unwrap();
}

#[tokio::test]
async fn bounded_reader_allows_crlf_frame_at_payload_limit() {
    let input = format!("{}\r\nnext\n", "x".repeat(32));
    let mut reader = BufReader::new(input.as_bytes());

    match read_bounded_line(&mut reader, 32).await.unwrap() {
        TcpFrame::Line(line) => assert_eq!(line, "x".repeat(32)),
        other => panic!("expected bounded CRLF line, got unexpected frame: {other:?}"),
    }

    match read_bounded_line(&mut reader, 32).await.unwrap() {
        TcpFrame::Line(line) => assert_eq!(line, "next"),
        other => panic!("expected next line after CRLF frame, got unexpected frame: {other:?}"),
    }
}
