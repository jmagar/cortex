use super::*;

#[test]
fn parse_config_set_accepts_key_value_equals_form_and_toml_path() {
    let args = strings(&["--toml", "--toml-path", "custom.toml", "syslog.port=1514"]);

    let parsed = parse_config_set(&args).unwrap();

    assert_eq!(parsed.target, crate::cli::ConfigTarget::Toml);
    assert_eq!(
        parsed.toml_path.unwrap(),
        std::path::PathBuf::from("custom.toml")
    );
    assert_eq!(parsed.key, "syslog.port");
    assert_eq!(parsed.value, "1514");
}

#[test]
fn parse_config_flags_rejects_env_and_toml_together() {
    let args = strings(&["--env", "--toml", "KEY"]);
    let mut target = crate::cli::ConfigTarget::Auto;
    let mut toml_path = None;
    let mut json = false;
    let mut positionals = Vec::new();

    let err = parse_config_flags(
        &args,
        &mut target,
        &mut toml_path,
        &mut json,
        &mut positionals,
        "get",
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("mutually exclusive"));
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
