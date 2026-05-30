use super::*;

#[test]
fn search_json_output_accepts_empty_response() {
    let response = cortex::app::SearchLogsResponse {
        logs: Vec::new(),
        count: 0,
    };

    print_search_response(&response, true).unwrap();
}
