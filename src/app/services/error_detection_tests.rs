use super::is_unaddressed_warning_noise;

#[test]
fn unaddressed_warning_noise_filters_health_checks_only() {
    assert!(is_unaddressed_warning_noise(
        "warning",
        "GET request for '/' received from 127.0.0.1 using 'curl/8.0'",
        "GET response status for '/' in 0.000 seconds plain 19 bytes: 302 Found",
    ));
    assert!(is_unaddressed_warning_noise(
        "warning",
        "tool list ok",
        "labby tool list ok in 44ms",
    ));
    assert!(!is_unaddressed_warning_noise(
        "err",
        "GET /health => generated",
        "GET /health => generated HTTP 200",
    ));
    assert!(!is_unaddressed_warning_noise(
        "warning",
        "imfile: cannot open file",
        "Permission denied reading /home/jmagar/.claude/projects/session.jsonl",
    ));
}
