use super::*;
use crate::cli::args::{AssessCommand, CliCommand};

#[test]
fn parse_assess_requires_subcommand() {
    let err = parse_assess(&[]).unwrap_err();
    assert!(format!("{err}").contains("assess requires a subcommand"));
}

#[test]
fn parse_assess_mcp_is_a_clear_not_yet_implemented_stub() {
    let err = parse_assess(&["mcp".to_string(), "some-tool".to_string()]).unwrap_err();
    assert!(format!("{err}").contains("not yet implemented"));
}

#[test]
fn parse_assess_hooks_is_a_clear_not_yet_implemented_stub() {
    let err = parse_assess(&["hooks".to_string()]).unwrap_err();
    assert!(format!("{err}").contains("not yet implemented"));
}

#[test]
fn parse_assess_skill_parses_positional_skill_name() {
    let cmd = parse_assess(&[
        "skill".to_string(),
        "cortex-frustration-assessment".to_string(),
    ])
    .unwrap();
    match cmd {
        CliCommand::Assess(AssessCommand::Skill(args)) => {
            assert_eq!(args.skill.as_deref(), Some("cortex-frustration-assessment"));
            assert_eq!(args.plugin, None);
            assert!(
                !args.no_llm,
                "LLM assessment must run by default (mirrors `cortex sessions assess`)"
            );
            assert!(!args.all);
            assert_eq!(args.limit, None);
        }
        other => panic!("expected AssessCommand::Skill, got {other:?}"),
    }
}

#[test]
fn parse_assess_skill_accepts_plugin_only() {
    let cmd = parse_assess(&[
        "skill".to_string(),
        "--plugin".to_string(),
        "lavra".to_string(),
    ])
    .unwrap();
    match cmd {
        CliCommand::Assess(AssessCommand::Skill(args)) => {
            assert_eq!(args.skill, None);
            assert_eq!(args.plugin.as_deref(), Some("lavra"));
        }
        other => panic!("expected AssessCommand::Skill, got {other:?}"),
    }
}

#[test]
fn parse_assess_skill_rejects_missing_positional_and_plugin() {
    let err = parse_assess(&["skill".to_string()]).unwrap_err();
    assert!(format!("{err}").contains("skill name or --plugin is required"));
}

#[test]
fn parse_assess_abuse_defaults_to_no_incident_id() {
    let cmd = parse_assess(&["abuse".to_string()]).unwrap();
    match cmd {
        CliCommand::Assess(AssessCommand::Abuse(args)) => {
            assert_eq!(args.incident_id, None);
            assert!(!args.no_llm);
        }
        other => panic!("expected AssessCommand::Abuse, got {other:?}"),
    }
}

#[test]
fn parse_assess_unknown_subcommand_suggests() {
    let err = parse_assess(&["bogus".to_string()]).unwrap_err();
    assert!(format!("{err}").contains("bogus"));
}
