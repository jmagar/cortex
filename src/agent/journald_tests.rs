use super::*;

#[test]
fn parse_entry_formats_journald_json_as_syslog_line() {
    let line = r#"{
        "MESSAGE": "disk almost full",
        "PRIORITY": "3",
        "SYSLOG_IDENTIFIER": "systemd",
        "_PID": "1234"
    }"#;

    let parsed = parse_entry("dookie", line).unwrap();

    assert!(parsed.starts_with("<131>1 "));
    assert!(parsed.contains(" dookie systemd 1234 - - disk almost full"));
}

#[test]
fn parse_entry_uses_systemd_unit_and_default_priority_when_fields_are_missing() {
    let line = r#"{
        "MESSAGE": "started service",
        "_SYSTEMD_UNIT": "cortex.service"
    }"#;

    let parsed = parse_entry("dookie", line).unwrap();

    assert!(parsed.starts_with("<134>1 "));
    assert!(parsed.contains(" dookie cortex.service - - - started service"));
}

#[test]
fn parse_entry_rejects_invalid_empty_or_messageless_entries() {
    assert_eq!(parse_entry("dookie", "not-json"), None);
    assert_eq!(parse_entry("dookie", r#"{"PRIORITY":"3"}"#), None);
    assert_eq!(parse_entry("dookie", r#"{"MESSAGE":"   "}"#), None);
}
