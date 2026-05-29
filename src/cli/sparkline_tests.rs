use super::sparkline_plain;

#[test]
fn sparkline_empty_returns_empty() {
    assert_eq!(sparkline_plain(&[]), "");
}

#[test]
fn sparkline_flat_returns_mid_level() {
    let out = sparkline_plain(&[5, 5, 5]);
    assert_eq!(out.chars().count(), 3);
    // All chars should be identical (flat line = mid-level block)
    let chars: Vec<char> = out.chars().collect();
    assert!(chars.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn sparkline_range_uses_all_levels() {
    let values: Vec<u64> = (0..8).collect();
    let out = sparkline_plain(&values);
    assert_eq!(out.chars().count(), 8);
    assert!(out.starts_with('▁'));
    assert!(out.ends_with('█'));
}
