use super::*;

#[test]
fn parse_timeline_collects_bucket_group_and_filters() {
    let args = strings(&[
        "--bucket",
        "hour",
        "--group-by",
        "hostname",
        "--hostname=host1",
        "--json",
    ]);

    let command = parse_timeline(&args).unwrap();

    match command {
        crate::cli::CliCommand::Timeline(args) => {
            assert_eq!(args.bucket.as_deref(), Some("hour"));
            assert_eq!(args.group_by.as_deref(), Some("hostname"));
            assert_eq!(args.hostname.as_deref(), Some("host1"));
            assert!(args.json);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_source_ips_accepts_limit_and_offset() {
    let args = strings(&["--limit", "10", "--offset=5"]);

    let command = parse_source_ips(&args).unwrap();

    match command {
        crate::cli::CliCommand::SourceIps(args) => {
            assert_eq!(args.limit, Some(10));
            assert_eq!(args.offset, Some(5));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
