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
fn parse_config_dispatches_all_subcommands_and_aliases() {
    let get = parse_config(&strings(&["get", "--json", "--env", "CORTEX_PORT"])).unwrap();
    let CliCommand::Config(ConfigCommand::Get(get)) = get else {
        panic!("expected get command");
    };
    assert!(get.json);
    assert_eq!(get.target, crate::cli::ConfigTarget::Env);
    assert_eq!(get.key, "CORTEX_PORT");

    let list = parse_config(&strings(&["ls", "--toml-path=config.toml"])).unwrap();
    let CliCommand::Config(ConfigCommand::List(list)) = list else {
        panic!("expected list command");
    };
    assert_eq!(
        list.toml_path.unwrap(),
        std::path::PathBuf::from("config.toml")
    );

    let unset = parse_config(&strings(&["unset", "--toml", "mcp.port"])).unwrap();
    let CliCommand::Config(ConfigCommand::Unset(unset)) = unset else {
        panic!("expected unset command");
    };
    assert_eq!(unset.target, crate::cli::ConfigTarget::Toml);
    assert_eq!(unset.key, "mcp.port");
}

#[test]
fn parse_config_set_accepts_split_key_value_and_empty_value_after_equals() {
    let parsed = parse_config_set(&strings(&["--env", "CORTEX_TOKEN", "secret"])).unwrap();
    assert_eq!(parsed.target, crate::cli::ConfigTarget::Env);
    assert_eq!(parsed.key, "CORTEX_TOKEN");
    assert_eq!(parsed.value, "secret");

    let parsed = parse_config_set(&strings(&["CORTEX_TOKEN="])).unwrap();
    assert_eq!(parsed.key, "CORTEX_TOKEN");
    assert_eq!(parsed.value, "");
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

#[test]
fn parse_config_reports_actionable_errors_for_bad_shapes() {
    for (args, expected) in [
        (vec!["bogus"], "unknown config subcommand"),
        (vec!["get"], "requires a KEY"),
        (vec!["get", "a", "b"], "exactly one KEY"),
        (vec!["set"], "requires KEY VALUE"),
        (vec!["set", "=value"], "KEY must not be empty"),
        (vec!["set", "a", "b", "c"], "too many positionals"),
        (vec!["unset"], "requires a KEY"),
        (vec!["unset", "a", "b"], "exactly one KEY"),
        (vec!["list", "extra"], "does not take positional"),
        (vec!["list", "--bogus"], "unknown config list option"),
        (vec!["list", "-h"], "use `cortex --help`"),
    ] {
        let err = parse_config(&strings(&args)).unwrap_err().to_string();
        assert!(
            err.contains(expected),
            "expected {err:?} to contain {expected:?}"
        );
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
