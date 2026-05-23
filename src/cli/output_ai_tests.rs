use super::*;

#[test]
fn checkpoint_json_output_is_accepted_for_empty_response() {
    print_checkpoints_response(&[], true).unwrap();
}

#[test]
fn parse_error_json_output_is_accepted_for_empty_response() {
    print_ai_parse_errors_response(&[], true).unwrap();
}
