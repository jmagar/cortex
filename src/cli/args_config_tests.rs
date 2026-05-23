use super::*;

#[test]
fn config_target_defaults_to_auto() {
    assert_eq!(ConfigGetArgs::default().target, ConfigTarget::Auto);
}
