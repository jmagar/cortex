use super::*;

#[test]
fn similar_incidents_json_output_accepts_empty_response() {
    let response = syslog_mcp::app::SimilarIncidentsResponse {
        query: "disk".to_string(),
        clusters: Vec::new(),
        total_clusters: 0,
        truncated: false,
    };

    print_similar_incidents_response(&response, true).unwrap();
}
