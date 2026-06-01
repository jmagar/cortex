use super::*;

/// Every top-level token the parser accepts (mirrors the `match` in
/// `src/cli/parse.rs` plus the Mode-level commands in `src/main.rs`). If a
/// command is added to the parser, add it here and to `CATALOG`.
const PARSER_TOKENS: &[&str] = &[
    // CliCommand::parse (src/cli/parse.rs)
    "search",
    "filter",
    "tail",
    "errors",
    "hosts",
    "sessions",
    "incident",
    "ai",
    "shell",
    "agent-command",
    "heartbeat",
    "correlate",
    "stats",
    "compose",
    "service",
    "setup",
    "db",
    "config",
    "source-ips",
    "timeline",
    "patterns",
    "ingest-rate",
    "sig",
    "notify",
    "silent-hosts",
    "clock-skew",
    "anomalies",
    "compare",
    "apps",
    "host-state",
    "fleet-state",
    "correlate-state",
    // Mode-level (src/main.rs)
    "serve",
    "mcp",
    "doctor",
    "deploy",
];

#[test]
fn catalog_covers_every_parser_token() {
    for token in PARSER_TOKENS {
        assert!(
            is_known(token),
            "parser accepts `{token}` but it has no CATALOG entry — add it to help.rs"
        );
    }
}

#[test]
fn every_catalog_entry_is_in_exactly_one_section() {
    for doc in CATALOG {
        let count = SECTIONS
            .iter()
            .filter(|(_, names)| names.contains(&doc.name))
            .count();
        assert_eq!(
            count, 1,
            "`{}` must appear in exactly one section, found {count}",
            doc.name
        );
    }
}

#[test]
fn every_section_name_is_a_real_command() {
    for (section, names) in SECTIONS {
        for name in *names {
            assert!(
                is_known(name),
                "section `{section}` lists `{name}` which is not in CATALOG"
            );
        }
    }
}

#[test]
fn top_level_help_plain_lists_sections_and_commands() {
    let out = render_top_level(false);
    assert!(!out.contains('\x1b'), "color=false must emit no ANSI");
    assert!(out.contains("CORTEX CLI"));
    assert!(out.contains("Quick Start"));
    assert!(out.contains("Commands"));
    assert!(out.contains("Search & Logs"));
    assert!(out.contains("source-ips"));
    assert!(out.contains("→ Run cortex <command> --help"));
}

#[test]
fn top_level_help_colored_emits_cyan_and_white() {
    let out = render_top_level(true);
    assert!(out.contains('\x1b'));
    assert!(out.contains("38;2;41;182;246"), "cyan headers");
    assert!(out.contains("38;2;230;244;251"), "white command names");
}

#[test]
fn command_help_shows_detailed_flags() {
    let out = render_command("db", false).expect("db is known");
    assert!(out.contains("--check-coord"), "db status flags present");
    assert!(out.contains("cortex db vacuum"));
    assert!(render_command("definitely-not-a-command", false).is_none());
}

#[test]
fn classify_help_distinguishes_top_level_command_and_none() {
    let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    assert_eq!(classify_help(&v(&["--help"])), HelpRequest::TopLevel);
    assert_eq!(classify_help(&v(&["help"])), HelpRequest::TopLevel);
    assert_eq!(classify_help(&v(&["-h"])), HelpRequest::TopLevel);
    assert_eq!(
        classify_help(&v(&["db", "status", "--help"])),
        HelpRequest::Command("db".to_string())
    );
    assert_eq!(
        classify_help(&v(&["search", "--help"])),
        HelpRequest::Command("search".to_string())
    );
    // Unknown command with a help flag falls back to the top-level banner.
    assert_eq!(
        classify_help(&v(&["bogus", "--help"])),
        HelpRequest::TopLevel
    );
    // No help token → not a help request.
    assert_eq!(classify_help(&v(&["search", "foo"])), HelpRequest::None);
    assert_eq!(classify_help(&v(&[])), HelpRequest::None);

    // Regression: the bare word `help` as a free-text QUERY (not in command
    // position) must NOT trigger help — these run the actual search.
    assert_eq!(classify_help(&v(&["search", "help"])), HelpRequest::None);
    assert_eq!(
        classify_help(&v(&["ai", "search", "help"])),
        HelpRequest::None
    );
    assert_eq!(
        classify_help(&v(&["ai", "abuse", "--term", "help"])),
        HelpRequest::None
    );
    // But `help` in command position is still a top-level request.
    assert_eq!(classify_help(&v(&["help"])), HelpRequest::TopLevel);
    assert_eq!(
        classify_help(&v(&["help", "search"])),
        HelpRequest::Command("search".to_string())
    );
    // `--` sentinel hides a wrapped command's --help.
    assert_eq!(
        classify_help(&v(&[
            "agent-command",
            "wrap",
            "--spool",
            "/x",
            "--",
            "tool",
            "--help"
        ])),
        HelpRequest::None
    );
}
