use super::*;

#[test]
fn graph_safe_display_escapes_terminal_control_characters() {
    assert_eq!(safe_display("host\x1b[31m\nnext"), "host\\u{1b}[31m\\nnext");
}

#[test]
fn graph_entity_json_output_accepts_empty_candidate_response() {
    let response = GraphEntityLookupResponse {
        resolved_entity: None,
        candidates: Vec::new(),
        metadata: cortex::app::GraphResponseMetadata {
            truncated: false,
            truncated_reason: None,
            limit: 20,
            depth: 0,
            evidence_sample_limit: 3,
            payload_budget: 32_768,
            projection_status: "empty".into(),
            last_completed_at: None,
            source_watermark: "logs:0;heartbeats:0;signatures:0".into(),
            last_error: None,
            is_degraded: false,
        },
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
        metadata: cortex::app::GraphResponseMetadata {
            truncated: false,
            truncated_reason: None,
            limit: 20,
            depth: 2,
            evidence_sample_limit: 2,
            payload_budget: 32_768,
            projection_status: "empty".into(),
            last_completed_at: None,
            source_watermark: "logs:0;heartbeats:0;signatures:0".into(),
            last_error: None,
            is_degraded: false,
        },
    };

    print_graph_explain_response(&response, true).unwrap();
}
