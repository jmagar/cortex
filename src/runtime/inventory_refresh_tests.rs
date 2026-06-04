use super::*;

#[test]
fn inventory_refresh_interval_parser_accepts_zero_disable() {
    assert_eq!(parse_inventory_refresh_interval_secs("0"), Some(0));
    assert_eq!(parse_inventory_refresh_interval_secs("300"), Some(300));
    assert_eq!(parse_inventory_refresh_interval_secs(" nope "), None);
}
