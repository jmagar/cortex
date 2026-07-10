use super::*;
use std::io::Write;

fn write_file(path: &Path, content: &str) {
    let mut file = fs::File::create(path).unwrap();
    file.write_all(content.as_bytes()).unwrap();
}

#[test]
fn collect_files_finds_supported_and_skips_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude/projects/foo");
    fs::create_dir_all(&claude_dir).unwrap();
    write_file(&claude_dir.join("session.jsonl"), "{}\n");
    write_file(&claude_dir.join("readme.txt"), "not a transcript\n");

    let mut out = Vec::new();
    collect_files(dir.path(), &mut out);
    assert_eq!(out.len(), 1);
    assert!(out[0].ends_with("session.jsonl"));
}

#[test]
fn read_new_lines_returns_only_lines_past_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    write_file(&path, "line0\nline1\nline2\n");

    let (lines, total) = read_new_lines(&path, 1, 500).unwrap();
    assert_eq!(total, 3);
    assert_eq!(
        lines,
        vec![(1, "line1".to_string()), (2, "line2".to_string())]
    );
}

#[test]
fn read_new_lines_respects_limit_and_reports_checkpoint_at_cutoff_not_eof() {
    // Regression: the checkpoint returned must reflect how far the limited
    // read actually got, not the file's true EOF — otherwise lines past the
    // limit are silently skipped forever on the next call.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    write_file(&path, "line0\nline1\nline2\nline3\nline4\n");

    let (lines, checkpoint) = read_new_lines(&path, 0, 2).unwrap();
    assert_eq!(
        lines,
        vec![(0, "line0".to_string()), (1, "line1".to_string())]
    );
    assert_eq!(
        checkpoint, 2,
        "checkpoint must stop at the limit, not report EOF (5)"
    );

    let (lines, checkpoint) = read_new_lines(&path, checkpoint, 2).unwrap();
    assert_eq!(
        lines,
        vec![(2, "line2".to_string()), (3, "line3".to_string())]
    );
    assert_eq!(checkpoint, 4);
}

#[test]
fn checkpoint_round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let checkpoint_path = dir.path().join("checkpoint.json");
    let mut checkpoint = Checkpoint::default();
    checkpoint.files.insert("/tmp/foo.jsonl".to_string(), 42);
    save_checkpoint(&checkpoint_path, &checkpoint).unwrap();

    let loaded = load_checkpoint(&checkpoint_path);
    assert_eq!(loaded.files.get("/tmp/foo.jsonl"), Some(&42));
}

#[tokio::test]
async fn scan_and_forward_sends_new_lines_and_advances_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude/projects/foo");
    fs::create_dir_all(&claude_dir).unwrap();
    let transcript_path = claude_dir.join("session.jsonl");
    write_file(
        &transcript_path,
        &format!(
            "{}\n",
            serde_json::json!({
                "type": "user",
                "timestamp": "2026-07-09T00:00:00Z",
                "sessionId": "sess-1",
                "message": {"role": "user", "content": "hello world"}
            })
        ),
    );

    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/ai-transcripts"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"accepted": 1})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let config = AiTranscriptForwardConfig {
        roots: vec![dir.path().to_path_buf()],
        target: server.uri(),
        token: Some("test-token".to_string()),
        hostname: "test-host".to_string(),
        checkpoint_path: dir.path().join("checkpoint.json"),
        poll_interval: Duration::from_secs(15),
    };
    let client = reqwest::Client::new();
    let mut checkpoint = Checkpoint::default();
    let sent = scan_and_forward(&config, &client, &mut checkpoint)
        .await
        .unwrap();
    assert_eq!(sent, 1);
    assert_eq!(
        checkpoint
            .files
            .get(&transcript_path.to_string_lossy().to_string()),
        Some(&1)
    );

    // Second scan with no new lines should send nothing.
    let sent_again = scan_and_forward(&config, &client, &mut checkpoint)
        .await
        .unwrap();
    assert_eq!(sent_again, 0);
}

#[tokio::test]
async fn scan_and_forward_scrubs_credentials_before_sending() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude/projects/foo");
    fs::create_dir_all(&claude_dir).unwrap();
    write_file(
        &claude_dir.join("session.jsonl"),
        &format!(
            "{}\n",
            serde_json::json!({
                "type": "user",
                "timestamp": "2026-07-09T00:00:00Z",
                "sessionId": "sess-1",
                "message": {"role": "user", "content": "export OPENAI_API_KEY=sk-proj-super-secret-value-long-enough-to-match"}
            })
        ),
    );

    let server = wiremock::MockServer::start().await;
    let received = std::sync::Arc::new(std::sync::Mutex::new(None));
    let received_clone = received.clone();
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/ai-transcripts"))
        .respond_with(move |req: &wiremock::Request| {
            *received_clone.lock().unwrap() = Some(req.body.clone());
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"accepted": 1}))
        })
        .expect(1)
        .mount(&server)
        .await;

    let config = AiTranscriptForwardConfig {
        roots: vec![dir.path().to_path_buf()],
        target: server.uri(),
        token: None,
        hostname: "test-host".to_string(),
        checkpoint_path: dir.path().join("checkpoint.json"),
        poll_interval: Duration::from_secs(15),
    };
    let client = reqwest::Client::new();
    let mut checkpoint = Checkpoint::default();
    scan_and_forward(&config, &client, &mut checkpoint)
        .await
        .unwrap();

    let body = received.lock().unwrap().take().unwrap();
    let body_str = String::from_utf8(body).unwrap();
    assert!(
        !body_str.contains("sk-proj-super-secret-value-long-enough-to-match"),
        "raw API key must not reach the network: {body_str}"
    );
    assert!(body_str.contains("REDACTED"), "got: {body_str}");
}

#[tokio::test]
async fn scan_and_forward_handles_gemini_whole_file_session_with_record_index_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let gemini_dir = dir.path().join(".gemini/tmp/abc123/chats");
    fs::create_dir_all(&gemini_dir).unwrap();
    let session_path = gemini_dir.join("session-1.json");
    write_file(
        &session_path,
        &serde_json::json!({
            "sessionId": "gemini-sess-1",
            "cwd": "/home/jmagar/workspace/cortex",
            "messages": [
                {"id": "m1", "timestamp": "2026-07-09T00:00:00Z", "content": "first message"},
            ]
        })
        .to_string(),
    );

    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/ai-transcripts"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"accepted": 1})),
        )
        .mount(&server)
        .await;

    let config = AiTranscriptForwardConfig {
        roots: vec![dir.path().to_path_buf()],
        target: server.uri(),
        token: None,
        hostname: "test-host".to_string(),
        checkpoint_path: dir.path().join("checkpoint.json"),
        poll_interval: Duration::from_secs(15),
    };
    let client = reqwest::Client::new();
    let mut checkpoint = Checkpoint::default();

    let sent = scan_and_forward(&config, &client, &mut checkpoint)
        .await
        .unwrap();
    assert_eq!(sent, 1);
    assert_eq!(
        checkpoint
            .files
            .get(&session_path.to_string_lossy().to_string()),
        Some(&1),
        "gemini checkpoint tracks a record index, not a byte offset"
    );

    // No new messages yet: re-scanning must send nothing.
    let sent_again = scan_and_forward(&config, &client, &mut checkpoint)
        .await
        .unwrap();
    assert_eq!(sent_again, 0);

    // Gemini rewrites the whole file with the new message appended —
    // only the new one (past the checkpoint) should forward next cycle.
    write_file(
        &session_path,
        &serde_json::json!({
            "sessionId": "gemini-sess-1",
            "cwd": "/home/jmagar/workspace/cortex",
            "messages": [
                {"id": "m1", "timestamp": "2026-07-09T00:00:00Z", "content": "first message"},
                {"id": "m2", "timestamp": "2026-07-09T00:01:00Z", "content": "second message"},
            ]
        })
        .to_string(),
    );
    let sent_third = scan_and_forward(&config, &client, &mut checkpoint)
        .await
        .unwrap();
    assert_eq!(sent_third, 1);
    assert_eq!(
        checkpoint
            .files
            .get(&session_path.to_string_lossy().to_string()),
        Some(&2)
    );
}
