use super::*;

fn log_entry(id: i64, message: &str) -> LogEntry {
    LogEntry {
        id,
        timestamp: "2026-06-13T12:00:00Z".to_string(),
        hostname: "dookie".to_string(),
        facility: Some("local0".to_string()),
        severity: "err".to_string(),
        app_name: Some("cortex".to_string()),
        process_id: Some("123".to_string()),
        message: message.to_string(),
        received_at: "2026-06-13T12:00:01Z".to_string(),
        source_ip: "127.0.0.1".to_string(),
        ai_tool: Some("codex".to_string()),
        ai_project: Some("/home/jmagar/workspace/cortex".to_string()),
        ai_session_id: Some("sess-1".to_string()),
        ai_transcript_path: None,
        metadata_json: None,
    }
}

fn abuse_incident() -> cortex::app::AbuseIncident {
    cortex::app::AbuseIncident {
        incident_id: "incident-1".to_string(),
        project: "/home/jmagar/workspace/cortex".to_string(),
        tool: "codex".to_string(),
        session_id: "sess-1".to_string(),
        hostname: "dookie".to_string(),
        first_seen: "2026-06-13T12:00:00Z".to_string(),
        last_seen: "2026-06-13T12:01:00Z".to_string(),
        duration_secs: 60,
        abuse_count: 1,
        terms: vec!["panic".to_string()],
        anchor_ids: vec![1],
        priority_score: 42.0,
        priority_label: "high".to_string(),
        window_minutes: 15,
    }
}

fn investigate_response() -> AiInvestigateResponse {
    AiInvestigateResponse {
        total_incidents: 2,
        truncated: true,
        evidence: vec![cortex::app::IncidentEvidence {
            incident: abuse_incident(),
            transcript_before: vec![log_entry(10, "before transcript window")],
            transcript_before_truncated: true,
            transcript_after: vec![log_entry(11, "after transcript window")],
            transcript_after_truncated: false,
            anchors: vec![log_entry(1, "anchor message with plenty of bytes")],
            nearby_logs: vec![log_entry(2, "nearby non-error log")],
            nearby_logs_truncated: true,
            nearby_errors: vec![log_entry(3, "nearby error message")],
            findings: cortex::app::IncidentFindings::default(),
        }],
    }
}

#[test]
fn similar_incidents_json_output_accepts_empty_response() {
    let response = cortex::app::SimilarIncidentsResponse {
        query: "disk".to_string(),
        clusters: Vec::new(),
        total_clusters: 0,
        truncated: false,
    };

    print_similar_incidents_response(&response, true).unwrap();
}

#[test]
fn compact_ai_investigate_json_omits_transcript_by_default_and_preserves_counts() {
    let value = compact_ai_investigate_json(
        &investigate_response(),
        AiInvestigatePrintOptions {
            detail: AiOutputDetail::Compact,
            include_transcript: false,
            max_bytes: 12,
        },
    );

    let evidence = &value["evidence"][0];
    assert_eq!(value["total_incidents"], 2);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["include_transcript"], false);
    assert_eq!(evidence["counts"]["anchors"], 1);
    assert_eq!(evidence["counts"]["transcript_before"], 1);
    assert_eq!(evidence["counts"]["nearby_logs"], 1);
    assert_eq!(evidence["truncated"]["transcript_before"], true);
    assert_eq!(evidence["truncated"]["nearby_logs"], true);
    assert!(evidence.get("transcript_before").is_none());
    assert!(evidence.get("transcript_after").is_none());
    let anchor_message = evidence["anchors"][0]["message"].as_str().unwrap();
    assert!(anchor_message.starts_with("anchor me"));
    assert_ne!(anchor_message, "anchor message with plenty of bytes");
}

#[test]
fn compact_ai_investigate_json_includes_transcript_when_requested() {
    let value = compact_ai_investigate_json(
        &investigate_response(),
        AiInvestigatePrintOptions {
            detail: AiOutputDetail::Compact,
            include_transcript: true,
            max_bytes: 64,
        },
    );

    let evidence = &value["evidence"][0];
    assert_eq!(value["include_transcript"], true);
    assert_eq!(
        evidence["transcript_before"][0]["message"],
        "before transcript window"
    );
    assert_eq!(
        evidence["transcript_after"][0]["message"],
        "after transcript window"
    );
    assert_eq!(
        evidence["nearby_errors"][0]["message"],
        "nearby error message"
    );
}
