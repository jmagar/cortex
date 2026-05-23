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
fn parse_compose_mutation_marks_down_as_non_interactive() {
    let args = strings(&["--yes", "--dry-run"]);

    let parsed = parse_compose_mutation(&args, true).unwrap();

    assert!(parsed.options.yes);
    assert!(parsed.options.dry_run);
    assert!(parsed.options.non_interactive);
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
