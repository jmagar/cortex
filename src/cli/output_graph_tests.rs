use super::*;

fn graph_metadata() -> cortex::app::GraphResponseMetadata {
    cortex::app::GraphResponseMetadata {
        truncated: false,
        truncated_reason: None,
        limit: 20,
        depth: 1,
        evidence_sample_limit: 2,
        payload_budget: 32_768,
        projection_status: "ready".into(),
        last_completed_at: None,
        source_watermark: "logs:1;heartbeats:0;signatures:0".into(),
        last_error: None,
        is_degraded: false,
    }
}

#[test]
fn graph_safe_display_escapes_terminal_control_characters() {
    assert_eq!(safe_display("host\x1b[31m\nnext"), "host\\u{1b}[31m\\nnext");
}

#[test]
fn graph_entity_json_output_accepts_empty_candidate_response() {
    let response = GraphEntityLookupResponse {
        resolved_entity: None,
        candidates: Vec::new(),
        metadata: graph_metadata(),
    };

    print_graph_entity_lookup_response(&response, true).unwrap();
}

#[test]
fn graph_explain_json_output_accepts_empty_response() {
    let response = GraphExplainResponse {
        resolved_entity: None,
        narrative: None,
        chains: Vec::new(),
        evidence: Vec::new(),
        open_questions: vec!["what changed?".into()],
        missing_evidence: vec!["relationship evidence".into()],
        next_queries: Vec::new(),
        candidates: Vec::new(),
        metadata: graph_metadata(),
    };

    print_graph_explain_response(&response, true).unwrap();
}

#[test]
fn graph_evidence_json_output_accepts_safe_response() {
    let src = cortex::app::GraphEntitySummary {
        id: 1,
        entity_type: "app".into(),
        canonical_key: "sshd".into(),
        display_label: "sshd".into(),
        trust_level: "verified".into(),
    };
    let dst = cortex::app::GraphEntitySummary {
        id: 2,
        entity_type: "host".into(),
        canonical_key: "host-a".into(),
        display_label: "host-a".into(),
        trust_level: "claimed".into(),
    };
    let response = cortex::app::GraphEvidenceLookupResponse {
        evidence: cortex::app::GraphEvidence {
            id: 9,
            relationship_id: 7,
            source_kind: "log".into(),
            source_id: "log:1".into(),
            source_log_id: Some(1),
            source_heartbeat_id: None,
            source_signature_hash: None,
            observed_at: "2026-01-01T00:00:00Z".into(),
            reason_code: "log_app_name".into(),
            reason_text: Some("app emitted log".into()),
            confidence_delta: 0.1,
            trust_level: "verified".into(),
            safe_excerpt: Some("accepted connection".into()),
            metadata_path: Some("$.app_name".into()),
            evidence_count: 1,
        },
        relationship: cortex::app::GraphRelationship {
            id: 7,
            relationship_key: "app:sshd->host:host-a".into(),
            src_entity_id: 1,
            dst_entity_id: 2,
            src_entity: Some(src.clone()),
            dst_entity: Some(dst.clone()),
            relationship_type: "emitted_by".into(),
            reason_code: "log_app_name".into(),
            trust_level: "verified".into(),
            confidence: 0.9,
            evidence_count: 1,
            evidence_ids: vec![9],
            first_seen_at: None,
            last_seen_at: None,
        },
        src_entity: src,
        dst_entity: dst,
        source_log_summary: Some(cortex::app::GraphSourceLogSummary {
            id: 1,
            timestamp: "2026-01-01T00:00:00Z".into(),
            received_at: "2026-01-01T00:00:00Z".into(),
            hostname: "host-a".into(),
            severity: "info".into(),
            app_name: Some("sshd".into()),
            process_id: None,
            source_ip: "10.0.0.1:514".into(),
            message: "line with \\u{1b} escaped".into(),
            message_truncated: false,
        }),
        missing_source_reason: None,
        metadata: graph_metadata(),
    };

    print_graph_evidence_lookup_response(&response, true).unwrap();
    print_graph_evidence_lookup_response(&response, false).unwrap();
}
