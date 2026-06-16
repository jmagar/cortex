use super::*;

#[test]
fn parse_db_vacuum_collects_full_force_pages_and_json() {
    let args = strings(&["--full", "--force", "--pages", "42", "--json"]);

    let command = parse_db_vacuum(&args).unwrap();

    match command {
        crate::cli::CliCommand::Db(crate::cli::DbCommand::Vacuum(args)) => {
            assert!(args.full);
            assert!(args.force);
            assert_eq!(args.pages, 42);
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_service_and_db_commands_dispatch_expected_subcommands() {
    let service = parse_service(&strings(&[
        "logs",
        "cortex",
        "--since=t0",
        "--until=t1",
        "--tail=20",
        "--json",
    ]))
    .unwrap();
    match service {
        crate::cli::CliCommand::Service(crate::cli::ServiceCommand::Logs(args)) => {
            assert_eq!(args.service, "cortex");
            assert_eq!(args.since.as_deref(), Some("t0"));
            assert_eq!(args.tail, Some(20));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let status = parse_db(&strings(&["status", "--check-coord", "--json"])).unwrap();
    match status {
        crate::cli::CliCommand::Db(crate::cli::DbCommand::Status(args)) => {
            assert!(args.check_coord);
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let integrity = parse_db(&strings(&["integrity", "--quick", "--background"])).unwrap();
    match integrity {
        crate::cli::CliCommand::Db(crate::cli::DbCommand::Integrity(args)) => {
            assert!(args.quick);
            assert!(args.background);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let integrity_status = parse_db(&strings(&["integrity", "status", "42", "--json"])).unwrap();
    match integrity_status {
        crate::cli::CliCommand::Db(crate::cli::DbCommand::IntegrityStatus(args)) => {
            assert_eq!(args.job_id, 42);
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let checkpoint = parse_db(&strings(&["checkpoint", "--mode=truncate", "--json"])).unwrap();
    match checkpoint {
        crate::cli::CliCommand::Db(crate::cli::DbCommand::Checkpoint(args)) => {
            assert_eq!(args.mode, "truncate");
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let backup = parse_db(&strings(&["backup", "--output=/tmp/cortex.db", "--json"])).unwrap();
    match backup {
        crate::cli::CliCommand::Db(crate::cli::DbCommand::Backup(args)) => {
            assert_eq!(args.output.as_deref(), Some("/tmp/cortex.db"));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_compose_mutation_marks_down_as_non_interactive() {
    let args = strings(&["--yes", "--dry-run"]);

    let parsed = parse_compose_mutation(&args, true).unwrap();

    assert!(parsed.options.yes);
    assert!(parsed.options.dry_run);
    assert!(parsed.options.non_interactive);
}

#[test]
fn parse_compose_commands_capture_targets_and_options() {
    let status = parse_compose(&strings(&[
        "status",
        "--compose-file=/srv/cortex/docker-compose.yml",
        "--project-dir=/srv/cortex",
        "--project-name=cortex",
        "--service=cortex",
        "--container=cortex-1",
        "--json",
    ]))
    .unwrap();
    match status {
        crate::cli::CliCommand::Compose(crate::cli::ComposeCommand::Status(args)) => {
            assert_eq!(args.target.project_name.as_deref(), Some("cortex"));
            assert_eq!(args.target.service.as_deref(), Some("cortex"));
            assert_eq!(args.target.container_name.as_deref(), Some("cortex-1"));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let logs = parse_compose(&strings(&["logs", "--tail=25", "--service", "cortex"])).unwrap();
    match logs {
        crate::cli::CliCommand::Compose(crate::cli::ComposeCommand::Logs(args)) => {
            assert_eq!(args.tail, Some(25));
            assert_eq!(args.target.service.as_deref(), Some("cortex"));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let up = parse_compose(&strings(&["up", "--allow-cwd-target", "--dry-run"])).unwrap();
    match up {
        crate::cli::CliCommand::Compose(crate::cli::ComposeCommand::Up(args)) => {
            assert!(args.options.allow_cwd_target);
            assert!(args.options.dry_run);
            assert!(!args.options.non_interactive);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_setup_and_plugin_hook_commands_accept_json_flags() {
    let check = parse_setup(&strings(&["check", "--json"])).unwrap();
    match check {
        crate::cli::CliCommand::Setup(crate::cli::SetupCommand::Check(args)) => {
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let hook = parse_setup(&strings(&["hook", "--json", "--no-repair"])).unwrap();
    match hook {
        crate::cli::CliCommand::Setup(crate::cli::SetupCommand::PluginHook(args)) => {
            assert!(args.json);
            assert!(args.no_repair);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_admin_commands_report_validation_errors() {
    for (parser, args, expected) in [
        (
            parse_service as fn(&[String]) -> anyhow::Result<crate::cli::CliCommand>,
            vec![],
            "service requires a subcommand",
        ),
        (parse_service, vec!["bogus"], "unknown service subcommand"),
        (
            parse_service_logs,
            vec!["--json"],
            "requires a service name",
        ),
        (parse_db, vec!["bogus"], "unknown db subcommand"),
        (parse_db_integrity_status, vec![], "requires a job id"),
        (parse_db_integrity_status, vec!["nope"], "invalid job id"),
        (
            parse_db_checkpoint,
            vec!["--mode=invalid"],
            "--mode must be one of",
        ),
        (parse_db_vacuum, vec!["--pages=0"], "greater than zero"),
        (parse_compose, vec!["config"], "deferred"),
        (parse_compose, vec!["upgrade"], "deferred"),
        (
            parse_compose_logs_as_command,
            vec!["--follow"],
            "--follow is deferred",
        ),
        (parse_setup, vec!["bogus"], "unknown setup subcommand"),
        (
            parse_plugin_hook_args_as_command,
            vec!["--bogus"],
            "unknown setup plugin-hook option",
        ),
    ] {
        let err = parser(&strings(&args)).unwrap_err().to_string();
        assert!(err.contains(expected), "expected {expected:?}, got {err:?}");
    }
}

fn parse_plugin_hook_args_as_command(args: &[String]) -> anyhow::Result<crate::cli::CliCommand> {
    parse_plugin_hook_args(args)
        .map(crate::cli::SetupCommand::PluginHook)
        .map(crate::cli::CliCommand::Setup)
}

fn parse_compose_logs_as_command(args: &[String]) -> anyhow::Result<crate::cli::CliCommand> {
    parse_compose_logs(args)
        .map(crate::cli::ComposeCommand::Logs)
        .map(crate::cli::CliCommand::Compose)
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
