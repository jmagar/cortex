use super::*;

#[test]
fn parse_meminfo_is_optional_and_non_panicking() {
    let _ = parse_meminfo();
}
