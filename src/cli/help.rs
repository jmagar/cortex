//! Top-level and per-command help, rendered in the Aurora-styled grouped
//! layout (modeled on `axon help`).
//!
//! Cortex is hand-rolled (no clap), so the command catalog is the single
//! source of truth here: each [`CommandDoc`] carries a one-line `summary` for
//! the grouped top-level listing and the detailed `usage` lines for
//! `cortex <command> --help`. The `catalog_covers_every_parser_token` test
//! guards against a command being added to the parser but not documented here.
//!
//! Color is built directly from the exported `*_ANSI` consts based on a `color`
//! flag the caller resolves once (via [`super::color`]) — so the render
//! functions stay pure and testable with `color = false`.

use super::color::{self, CYAN_ANSI, MUTED_ANSI, PRIMARY_ANSI};

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";

const TAGLINE: &str = "Syslog intelligence for homelabs";

/// One documented top-level command (or namespace root).
struct CommandDoc {
    /// Top-level token as typed, e.g. `search`, `ai`, `db`.
    name: &'static str,
    /// One-line description for the grouped top-level listing.
    summary: &'static str,
    /// Detailed invocation/flag lines for `cortex <name> --help`.
    usage: &'static [&'static str],
}

struct NestedCommandDoc {
    /// Full path after `cortex`, e.g. `ai search`.
    path: &'static str,
    summary: &'static str,
    usage: &'static [&'static str],
}

/// Section title → ordered command names. Drives the grouped `Commands` block;
/// every catalog entry must appear in exactly one section.
const SECTIONS: &[(&str, &[&str])] = &[
    (
        "Search & Logs",
        &[
            "search",
            "filter",
            "tail",
            "errors",
            "hosts",
            "sessions",
            "incident",
            "source-ips",
            "entity",
            "graph",
        ],
    ),
    (
        "Analytics & Correlation",
        &[
            "stats",
            "timeline",
            "patterns",
            "ingest-rate",
            "apps",
            "correlate",
            "host-state",
            "fleet-state",
            "correlate-state",
            "silent-hosts",
            "clock-skew",
            "anomalies",
            "compare",
        ],
    ),
    ("AI Transcripts", &["ai"]),
    ("Signals & Alerts", &["sig", "notify"]),
    ("Ingestion", &["shell", "agent-command", "heartbeat"]),
    (
        "Runtime & Setup",
        &[
            "serve", "mcp", "doctor", "db", "compose", "service", "setup", "deploy", "config",
        ],
    ),
];

const CATALOG: &[CommandDoc] = &[
    // ── Search & Logs ──────────────────────────────────────────────────────
    CommandDoc {
        name: "search",
        summary: "Full-text search across all logs",
        usage: &["cortex search [query] [--hostname HOST] [--source-ip SOURCE] [--severity LEVEL] [--app-name APP] [--facility FACILITY] [--exclude-facility FACILITY] [--from TIME] [--to TIME] [--received-from TIME] [--received-to TIME] [--limit N] [--json]"],
    },
    CommandDoc {
        name: "filter",
        summary: "Filter logs by structured fields (host, container, severity…)",
        usage: &["cortex filter [--hostname HOST] [--source-ip SOURCE] [--source-kind KIND] [--tool TOOL] [--project PATH] [--session-id ID] [--container NAME] [--docker-host HOST] [--stream stdout|stderr] [--event-action ACTION] [--severity LEVEL] [--app-name APP] [--facility FACILITY] [--exclude-facility FACILITY] [--from TIME] [--to TIME] [--received-from TIME] [--received-to TIME] [--limit N] [--json]"],
    },
    CommandDoc {
        name: "tail",
        summary: "Show the most recent log lines",
        usage: &["cortex tail [-n N] [--hostname HOST] [--source-ip SOURCE] [--app-name APP] [--json]"],
    },
    CommandDoc {
        name: "errors",
        summary: "Recent error-level log entries",
        usage: &["cortex errors [--from TIME] [--to TIME] [--limit N] [--json]"],
    },
    CommandDoc {
        name: "hosts",
        summary: "List all hosts that have sent logs",
        usage: &["cortex hosts [--json]"],
    },
    CommandDoc {
        name: "sessions",
        summary: "List indexed AI sessions",
        usage: &["cortex sessions [--project PATH] [--tool TOOL] [--hostname HOST] [--from TIME] [--to TIME] [--limit N] [--json]"],
    },
    CommandDoc {
        name: "incident",
        summary: "Logs surrounding a point in time",
        usage: &["cortex incident --around TIME [--minutes N] [--service SERVICE] [--host HOST] [--limit N] [--json]"],
    },
    CommandDoc {
        name: "source-ips",
        summary: "List unique source IPs with log counts",
        usage: &["cortex source-ips [--limit N] [--offset N] [--json]"],
    },
    CommandDoc {
        name: "entity",
        summary: "Resolve a graph entity by type/key or alias",
        usage: &[
            "cortex entity <entity-type> <key> [--limit N] [--json]",
            "cortex entity <entity-type:key> [--json]",
            "cortex entity --alias-type TYPE --alias-key KEY [--limit N] [--json]",
        ],
    },
    CommandDoc {
        name: "graph",
        summary: "Explore one-hop graph neighborhoods with evidence",
        usage: &[
            "cortex graph around <entity-type> <key> [--limit N] [--depth 1] [--evidence-sample-limit N] [--payload-budget BYTES] [--json]",
            "cortex graph around <entity-type:key> [--json]",
            "cortex graph around --entity-id ID [--limit N] [--json]",
            "cortex graph explain <entity-type> <key> [--depth 2] [--beam-width N] [--max-chains N] [--json]",
            "cortex graph explain <entity-type:key> [--json]",
            "cortex graph explain --entity-id ID [--depth 2] [--json]",
            "cortex graph evidence <evidence-id> [--payload-budget BYTES] [--json]",
            "cortex graph status [--json]",
            "cortex graph rebuild [--json]",
        ],
    },
    // ── Analytics & Correlation ────────────────────────────────────────────
    CommandDoc {
        name: "stats",
        summary: "Database and ingest statistics",
        usage: &["cortex stats [--json]"],
    },
    CommandDoc {
        name: "timeline",
        summary: "Log volume over time, bucketed",
        usage: &["cortex timeline [--bucket minute|hour|day] [--group-by FIELD] [--hostname HOST] [--app-name APP] [--severity-min LEVEL] [--from TIME] [--to TIME] [--json]"],
    },
    CommandDoc {
        name: "patterns",
        summary: "Recurring message patterns",
        usage: &["cortex patterns [--top-n N] [--scan-limit N] [--hostname HOST] [--app-name APP] [--severity-min LEVEL] [--from TIME] [--to TIME] [--json]"],
    },
    CommandDoc {
        name: "ingest-rate",
        summary: "Current ingest rate (logs/sec)",
        usage: &["cortex ingest-rate [--by-host] [--json]"],
    },
    CommandDoc {
        name: "apps",
        summary: "Top application/program names by volume",
        usage: &["cortex apps [--hostname HOST] [--from TIME] [--to TIME] [--limit N] [--offset N] [--json]"],
    },
    CommandDoc {
        name: "correlate",
        summary: "Correlate events around a reference time",
        usage: &[
            "cortex correlate --reference-time TIME [--window-minutes N] [--severity-min LEVEL] [--hostname HOST] [--source-ip SOURCE] [--query FTS] [--limit N] [--json]",
        ],
    },
    CommandDoc {
        name: "host-state",
        summary: "Per-host health/pressure snapshot",
        usage: &["cortex host-state [--host-id ID] [--hostname HOST] [--since TIME] [--limit N] [--json]"],
    },
    CommandDoc {
        name: "fleet-state",
        summary: "Fleet-wide host state overview",
        usage: &["cortex fleet-state [--include-ok|--exclude-ok] [--sort pressure|freshness|hostname] [--json]"],
    },
    CommandDoc {
        name: "correlate-state",
        summary: "Correlate host state at a reference time",
        usage: &["cortex correlate-state --reference-time TIME [--window-minutes N] [--host HOST] [--severity-min LEVEL] [--limit N] [--json]"],
    },
    CommandDoc {
        name: "silent-hosts",
        summary: "Hosts that have gone quiet",
        usage: &["cortex silent-hosts [--silent-minutes N] [--json]"],
    },
    CommandDoc {
        name: "clock-skew",
        summary: "Detect host clock skew",
        usage: &["cortex clock-skew [--since TIME] [--limit N] [--json]"],
    },
    CommandDoc {
        name: "anomalies",
        summary: "Log-volume anomalies vs a baseline window",
        usage: &["cortex anomalies [--recent-minutes N] [--baseline-minutes N] [--json]"],
    },
    CommandDoc {
        name: "compare",
        summary: "Compare two time windows",
        usage: &["cortex compare --a-from TIME --a-to TIME --b-from TIME --b-to TIME [--json]"],
    },
    // ── AI Transcripts ─────────────────────────────────────────────────────
    CommandDoc {
        name: "ai",
        summary: "AI transcript search, correlation, and indexing",
        usage: &[
            "cortex ai search QUERY [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--json]",
            "cortex ai abuse [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--before N] [--after N] [--term WORD] [--json]",
            "cortex ai incidents [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--window-minutes N] [--term WORD] [--json]",
            "cortex ai investigate [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--window-minutes N] [--correlation-window-minutes N] [--term WORD] [--detail compact|full] [--include-transcript] [--max-bytes N] [--json]",
            "cortex ai assess INCIDENT_ID [--model MODEL] [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--window-minutes N] [--correlation-window-minutes N] [--term WORD] [--json]",
            "cortex ai correlate [--project PATH] [--tool TOOL] [--session-id ID] [--ai-query FTS] [--log-query FTS] [--hostname HOST] [--source-ip SOURCE] [--app-name APP] [--from TIME] [--to TIME] [--window-minutes N] [--severity-min LEVEL] [--limit N] [--events-per-anchor N] [--json]",
            "cortex ai blocks [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--detail compact|full] [--json]",
            "cortex ai context --project PATH [--tool TOOL] [--limit N] [--json]",
            "cortex ai tools [--project PATH] [--from TIME] [--to TIME] [--json]",
            "cortex ai projects [--tool TOOL] [--from TIME] [--to TIME] [--json]",
            "cortex ai index [--path PATH] [--since TIME] [--force] [--json]",
            "cortex ai add --file FILE [--force] [--json]",
            "cortex ai watch [--path PATH] [--debounce-ms N] [--settle-ms N] [--max-retries N] [--no-initial-scan] [--json]",
            "cortex ai checkpoints [--errors] [--missing] [--limit N] [--json]",
            "cortex ai errors [--limit N] [--json]",
            "cortex ai prune-checkpoints --missing [--dry-run] [--limit N] [--json]",
            "cortex ai doctor [--strict-permissions] [--json]",
            "cortex ai watch-status [--json]",
            "cortex ai smoke-watch [--json]",
        ],
    },
    // ── Signals & Alerts ───────────────────────────────────────────────────
    CommandDoc {
        name: "sig",
        summary: "Manage error signatures (list/ack/unack)",
        usage: &[
            "cortex sig list [--include-acknowledged] [--limit N] [--json]",
            "cortex sig ack HASH [--notes TEXT] [--json]",
            "cortex sig unack HASH [--reason TEXT] [--json]",
        ],
    },
    CommandDoc {
        name: "notify",
        summary: "Notification firings and test sends",
        usage: &[
            "cortex notify recent [--rule-id ID] [--since TIME] [--limit N] [--json]",
            "cortex notify test [--body TEXT] [--json]   (requires --http)",
        ],
    },
    // ── Ingestion ──────────────────────────────────────────────────────────
    CommandDoc {
        name: "shell",
        summary: "Index shell history (zsh, atuin)",
        usage: &[
            "cortex shell index --path PATH [--shell zsh] [--json]",
            "cortex shell atuin-index --path PATH [--json]",
        ],
    },
    CommandDoc {
        name: "agent-command",
        summary: "Ingest and wrap agent command spools",
        usage: &[
            "cortex agent-command ingest-spool --path PATH [--json]",
            "cortex agent-command wrap --spool PATH -- COMMAND...",
        ],
    },
    CommandDoc {
        name: "heartbeat",
        summary: "Run the host heartbeat agent",
        usage: &["cortex heartbeat agent [--target URL] [--token TOKEN] [--interval-secs N] [--probe-deadline-ms N] [--collection-deadline-ms N] [--retry-buffer N] [--host-id-path PATH] [--once|--emit] [--json]"],
    },
    // ── Runtime & Setup ────────────────────────────────────────────────────
    CommandDoc {
        name: "serve",
        summary: "Start the full server (ingest + HTTP MCP)",
        usage: &["cortex serve mcp     Start syslog UDP/TCP ingest plus HTTP MCP server"],
    },
    CommandDoc {
        name: "mcp",
        summary: "Start query-only MCP stdio transport",
        usage: &["cortex mcp"],
    },
    CommandDoc {
        name: "doctor",
        summary: "Run all health checks (setup, compose, binary, AI)",
        usage: &["cortex doctor [--json]", "cortex doctor binary [--json]"],
    },
    CommandDoc {
        name: "db",
        summary: "Database maintenance (status/integrity/vacuum/backup)",
        usage: &[
            "cortex db status [--check-coord] [--json]",
            "cortex db integrity [--quick] [--json]",
            "cortex db checkpoint [--mode passive|full|restart|truncate] [--json]",
            "cortex db vacuum [--pages N|--full] [--force] [--json]",
            "cortex db backup [--output PATH] [--json]",
        ],
    },
    CommandDoc {
        name: "compose",
        summary: "Manage the Docker Compose stack",
        usage: &[
            "cortex compose doctor [--json]",
            "cortex compose status [--compose-file FILE] [--project-dir DIR] [--project-name NAME] [--json]",
            "cortex compose pull|up|restart [--dry-run] [--allow-cwd-target] [--json]",
            "cortex compose down --yes [--dry-run] [--allow-cwd-target] [--json]",
            "cortex compose logs [--tail N] [--json]",
        ],
    },
    CommandDoc {
        name: "service",
        summary: "Inspect container service logs",
        usage: &["cortex service logs SERVICE [--from TIME] [--to TIME] [--tail N] [--json]"],
    },
    CommandDoc {
        name: "setup",
        summary: "Initialize, check, and repair configuration",
        usage: &[
            "cortex setup check|repair [--json]",
            "cortex setup ai-index-timer install|remove|check [--json]",
            "cortex setup ai-watch-service install|remove|check [--json]",
            "cortex setup agent-command install|remove|check [--json]",
            "cortex setup heartbeat-agent install|remove|check [--json]",
            "cortex setup debug-wrapper install|remove|check [--json]",
            "cortex setup debug-compose install|remove|check [--json]",
            "cortex setup plugin-hook [--no-repair] [--json]",
            "cortex setup doctor [--json]",
        ],
    },
    CommandDoc {
        name: "deploy",
        summary: "Provision cortex locally or on remote hosts",
        usage: &[
            "cortex deploy preflight [--json]",
            "cortex deploy local [--dry-run] [--json]",
            "cortex deploy remote HOST [--dry-run] [--json]",
        ],
    },
    CommandDoc {
        name: "config",
        summary: "Read and write configuration entries",
        usage: &[
            "cortex config get KEY [--env|--toml] [--toml-path PATH] [--json]",
            "cortex config set KEY VALUE [--env|--toml] [--toml-path PATH] [--json]",
            "cortex config unset KEY [--env|--toml] [--toml-path PATH] [--json]",
            "cortex config list [--env|--toml] [--toml-path PATH] [--json]",
        ],
    },
];

const NESTED_CATALOG: &[NestedCommandDoc] = &[
    NestedCommandDoc {
        path: "ai search",
        summary: "Full-text search over indexed AI transcript sessions",
        usage: &["cortex ai search QUERY [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--json]"],
    },
    NestedCommandDoc {
        path: "ai abuse",
        summary: "Find risky or failure-related transcript messages",
        usage: &["cortex ai abuse [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--before N] [--after N] [--term WORD] [--json]"],
    },
    NestedCommandDoc {
        path: "ai incidents",
        summary: "Cluster AI transcript abuse matches into incidents",
        usage: &["cortex ai incidents [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--window-minutes N] [--term WORD] [--json]"],
    },
    NestedCommandDoc {
        path: "ai investigate",
        summary: "Expand AI incidents into evidence bundles",
        usage: &[
            "cortex ai investigate [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--window-minutes N] [--correlation-window-minutes N] [--term WORD] [--detail compact|full] [--include-transcript] [--max-bytes N] [--json]",
            "Default output is compact; use --detail full for complete evidence.",
        ],
    },
    NestedCommandDoc {
        path: "ai assess",
        summary: "Assess one AI incident with optional model context",
        usage: &["cortex ai assess INCIDENT_ID [--model MODEL] [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--window-minutes N] [--correlation-window-minutes N] [--term WORD] [--json]"],
    },
    NestedCommandDoc {
        path: "ai correlate",
        summary: "Correlate AI transcript anchors with non-AI logs",
        usage: &["cortex ai correlate [--project PATH] [--tool TOOL] [--session-id ID] [--ai-query FTS] [--log-query FTS] [--hostname HOST] [--source-ip SOURCE] [--app-name APP] [--from TIME] [--to TIME] [--window-minutes N] [--severity-min LEVEL] [--limit N] [--events-per-anchor N] [--json]"],
    },
    NestedCommandDoc {
        path: "ai blocks",
        summary: "AI transcript activity grouped into 5-hour UTC blocks",
        usage: &[
            "cortex ai blocks [--project PATH] [--tool TOOL] [--from TIME] [--to TIME] [--limit N] [--detail compact|full] [--json]",
            "Default output is capped for interactive use; use --detail full for every block.",
        ],
    },
    NestedCommandDoc {
        path: "ai context",
        summary: "Recent AI transcript context for one project",
        usage: &["cortex ai context --project PATH [--tool TOOL] [--limit N] [--json]"],
    },
    NestedCommandDoc {
        path: "ai tools",
        summary: "List AI tools present in transcript metadata",
        usage: &["cortex ai tools [--project PATH] [--from TIME] [--to TIME] [--json]"],
    },
    NestedCommandDoc {
        path: "ai projects",
        summary: "List AI projects present in transcript metadata",
        usage: &["cortex ai projects [--tool TOOL] [--from TIME] [--to TIME] [--json]"],
    },
    NestedCommandDoc {
        path: "ai index",
        summary: "Index local AI transcript roots",
        usage: &["cortex ai index [--path PATH] [--since TIME] [--force] [--json]"],
    },
    NestedCommandDoc {
        path: "ai add",
        summary: "Index one AI transcript file",
        usage: &["cortex ai add --file FILE [--force] [--json]"],
    },
    NestedCommandDoc {
        path: "ai watch",
        summary: "Run the local transcript watch daemon",
        usage: &["cortex ai watch [--path PATH] [--debounce-ms N] [--settle-ms N] [--max-retries N] [--no-initial-scan] [--json]"],
    },
    NestedCommandDoc {
        path: "ai checkpoints",
        summary: "List AI transcript indexing checkpoints",
        usage: &["cortex ai checkpoints [--errors] [--missing] [--limit N] [--json]"],
    },
    NestedCommandDoc {
        path: "ai errors",
        summary: "List AI transcript parse errors",
        usage: &["cortex ai errors [--limit N] [--json]"],
    },
    NestedCommandDoc {
        path: "ai prune-checkpoints",
        summary: "Prune stale AI indexing checkpoints",
        usage: &["cortex ai prune-checkpoints --missing [--dry-run] [--limit N] [--json]"],
    },
    NestedCommandDoc {
        path: "ai doctor",
        summary: "Check local AI transcript indexing prerequisites",
        usage: &["cortex ai doctor [--strict-permissions] [--json]"],
    },
    NestedCommandDoc {
        path: "ai watch-status",
        summary: "Inspect the local AI transcript watch service",
        usage: &["cortex ai watch-status [--json]"],
    },
    NestedCommandDoc {
        path: "ai smoke-watch",
        summary: "Run a local AI transcript watch smoke test",
        usage: &["cortex ai smoke-watch [--json]"],
    },
    NestedCommandDoc {
        path: "ai similar",
        summary: "Find incidents similar to a free-text query",
        usage: &["cortex ai similar QUERY [--hostname HOST] [--app-name APP] [--severity-min LEVEL] [--from TIME] [--to TIME] [--window-minutes N] [--limit N] [--json]"],
    },
    NestedCommandDoc {
        path: "ai ask-history",
        summary: "Search historical AI sessions and nearby system logs",
        usage: &["cortex ai ask-history QUERY [--hostname HOST] [--app-name APP] [--from TIME] [--to TIME] [--limit N] [--json]"],
    },
    NestedCommandDoc {
        path: "ai incident-context",
        summary: "Build incident context from an explicit time window",
        usage: &["cortex ai incident-context --from TIME --to TIME [--hostname HOST] [--app-name APP] [--query FTS] [--severity-min LEVEL] [--limit N] [--json]"],
    },
    NestedCommandDoc {
        path: "db backup",
        summary: "Create a WAL-safe SQLite backup",
        usage: &["cortex db backup [--output PATH] [--json]"],
    },
    NestedCommandDoc {
        path: "db status",
        summary: "Inspect SQLite maintenance state",
        usage: &["cortex db status [--check-coord] [--json]"],
    },
    NestedCommandDoc {
        path: "db integrity",
        summary: "Run SQLite integrity checks",
        usage: &["cortex db integrity [--quick] [--json]"],
    },
    NestedCommandDoc {
        path: "db checkpoint",
        summary: "Run a SQLite WAL checkpoint",
        usage: &["cortex db checkpoint [--mode passive|full|restart|truncate] [--json]"],
    },
    NestedCommandDoc {
        path: "db vacuum",
        summary: "Run SQLite incremental or full vacuum",
        usage: &["cortex db vacuum [--pages N] [--full] [--force] [--json]"],
    },
    NestedCommandDoc {
        path: "compose status",
        summary: "Inspect the resolved Docker Compose runtime",
        usage: &["cortex compose status [--compose-file PATH] [--project-dir DIR] [--project-name NAME] [--service NAME] [--container NAME] [--json]"],
    },
    NestedCommandDoc {
        path: "compose doctor",
        summary: "Diagnose Docker Compose/listener ownership",
        usage: &["cortex compose doctor [--compose-file PATH] [--project-dir DIR] [--project-name NAME] [--service NAME] [--container NAME] [--json]"],
    },
    NestedCommandDoc {
        path: "compose pull",
        summary: "Pull the resolved Docker Compose image",
        usage: &["cortex compose pull [--dry-run] [--allow-cwd-target] [--json]"],
    },
    NestedCommandDoc {
        path: "compose up",
        summary: "Recreate the resolved Docker Compose service",
        usage: &["cortex compose up [--dry-run] [--allow-cwd-target] [--json]"],
    },
    NestedCommandDoc {
        path: "compose restart",
        summary: "Restart the resolved Docker Compose service",
        usage: &["cortex compose restart [--dry-run] [--allow-cwd-target] [--json]"],
    },
    NestedCommandDoc {
        path: "compose down",
        summary: "Stop the resolved Docker Compose service",
        usage: &["cortex compose down --yes [--dry-run] [--allow-cwd-target] [--json]"],
    },
    NestedCommandDoc {
        path: "compose logs",
        summary: "Show bounded Docker Compose logs",
        usage: &["cortex compose logs [--tail N] [--json]"],
    },
    NestedCommandDoc {
        path: "setup check",
        summary: "Audit plugin setup without changing files",
        usage: &["cortex setup check [--json]"],
    },
    NestedCommandDoc {
        path: "setup repair",
        summary: "Repair plugin setup idempotently",
        usage: &["cortex setup repair [--json]"],
    },
    NestedCommandDoc {
        path: "setup install",
        summary: "Install plugin setup artifacts",
        usage: &["cortex setup install [--json]"],
    },
    NestedCommandDoc {
        path: "setup plugin-hook",
        summary: "Run plugin setup hook repair or audit mode",
        usage: &["cortex setup plugin-hook [--no-repair] [--json]"],
    },
    NestedCommandDoc {
        path: "setup doctor",
        summary: "Run setup diagnostics across all phases",
        usage: &["cortex setup doctor [--json]"],
    },
    NestedCommandDoc {
        path: "sig list",
        summary: "List error signatures",
        usage: &["cortex sig list [--include-acknowledged] [--limit N] [--json]"],
    },
    NestedCommandDoc {
        path: "sig ack",
        summary: "Acknowledge an error signature",
        usage: &["cortex sig ack HASH [--notes TEXT] [--json]"],
    },
    NestedCommandDoc {
        path: "sig unack",
        summary: "Unacknowledge an error signature",
        usage: &["cortex sig unack HASH [--reason TEXT] [--json]"],
    },
    NestedCommandDoc {
        path: "notify recent",
        summary: "List recent notification firings",
        usage: &["cortex notify recent [--rule-id ID] [--since TIME] [--limit N] [--json]"],
    },
    NestedCommandDoc {
        path: "notify test",
        summary: "Send a test notification",
        usage: &["cortex notify test [--body TEXT] [--json]   (requires --http)"],
    },
    NestedCommandDoc {
        path: "shell index",
        summary: "Index shell history",
        usage: &["cortex shell index --path PATH [--shell zsh] [--json]"],
    },
    NestedCommandDoc {
        path: "shell atuin-index",
        summary: "Index Atuin shell history",
        usage: &["cortex shell atuin-index --path PATH [--json]"],
    },
    NestedCommandDoc {
        path: "agent-command ingest-spool",
        summary: "Ingest agent command spool files",
        usage: &["cortex agent-command ingest-spool --path PATH [--json]"],
    },
    NestedCommandDoc {
        path: "agent-command wrap",
        summary: "Wrap a command and spool execution metadata",
        usage: &["cortex agent-command wrap --spool PATH -- COMMAND..."],
    },
    NestedCommandDoc {
        path: "heartbeat agent",
        summary: "Run the host heartbeat agent",
        usage: &["cortex heartbeat agent [--target URL] [--token TOKEN] [--interval-secs N] [--probe-deadline-ms N] [--collection-deadline-ms N] [--retry-buffer N] [--host-id-path PATH] [--once|--emit] [--json]"],
    },
];

const GLOBAL_OPTIONS: &[(&str, &str)] = &[
    ("-h, --help", "Display help (top-level or per-command)"),
    ("--version", "Print version and exit"),
    ("--color <when>", "Colorize output: always, never, or auto"),
    (
        "--no-color",
        "Disable colored output (alias for --color=never)",
    ),
    (
        "--http",
        "Route through the container REST API instead of local SQLite (fail-closed)",
    ),
    (
        "--server <URL>",
        "API base URL (implies --http). Default: CORTEX_URL or http://127.0.0.1:3100",
    ),
    (
        "--token <TOKEN>",
        "Bearer token (implies --http). Default: CORTEX_API_TOKEN",
    ),
];

const ENVIRONMENT: &[(&str, &str)] = &[
    (
        "CORTEX_DB_PATH",
        "SQLite database path used by both transports",
    ),
    (
        "CORTEX_USE_HTTP",
        "Set 1/true to default to HTTP mode (fail-closed if discovery fails)",
    ),
    (
        "CORTEX_URL",
        "Default API base URL for --http (overridden by --server)",
    ),
    (
        "CORTEX_API_TOKEN",
        "Bearer token for --http (overridden by --token)",
    ),
    ("RUST_LOG", "Log filter; stdio logs always go to stderr"),
];

const QUICK_START: &[&str] = &[
    "cortex search \"oom killer\" --hostname web-01",
    "cortex tail -n 50 --severity err",
    "cortex ai investigate --window-minutes 30",
];

// ── Color helpers (pure: driven by the `color` flag) ────────────────────────

fn paint(color: bool, code: &str, text: &str) -> String {
    if color {
        format!("{code}{text}{RESET}")
    } else {
        text.to_string()
    }
}

/// Bold cyan — section headers and category subheaders.
fn heading(color: bool, text: &str) -> String {
    if color {
        format!("{BOLD}{CYAN_ANSI}{text}{RESET}")
    } else {
        text.to_string()
    }
}

/// Aligned `label  description` row. `label_code` colors the label (white for
/// command names, cyan for flags); the description is muted. Wraps to a second
/// line when the label is wider than `label_width`.
fn push_row(
    out: &mut String,
    color: bool,
    indent: usize,
    label_width: usize,
    label_code: &str,
    label: &str,
    desc: &str,
) {
    if label.chars().count() > label_width {
        out.push_str(&format!(
            "{:indent$}{}\n",
            "",
            paint(color, label_code, label),
            indent = indent
        ));
        out.push_str(&format!(
            "{:width$}{}\n",
            "",
            paint(color, MUTED_ANSI, desc),
            width = indent + label_width + 1
        ));
        return;
    }
    let padded = format!("{label:<label_width$}");
    out.push_str(&format!(
        "{:indent$}{} {}\n",
        "",
        paint(color, label_code, &padded),
        paint(color, MUTED_ANSI, desc),
        indent = indent
    ));
}

/// Render the full top-level help banner.
pub(crate) fn render_top_level(color: bool) -> String {
    let mut out = String::with_capacity(4096);

    out.push_str(&format!("  {}\n", heading(color, "CORTEX CLI")));
    out.push_str(&format!(
        "  {}\n",
        paint(color, CYAN_ANSI, "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
    ));
    out.push_str(&format!(
        "  Version {}  |  {}\n\n",
        env!("CARGO_PKG_VERSION"),
        paint(color, MUTED_ANSI, TAGLINE)
    ));

    out.push_str(&format!("  {}\n", heading(color, "Usage")));
    out.push_str(&format!(
        "  {}\n\n",
        paint(color, CYAN_ANSI, "cortex [options] <command> [args]")
    ));

    out.push_str(&format!("  {}\n", heading(color, "Quick Start")));
    for example in QUICK_START {
        out.push_str(&format!("  {}\n", paint(color, MUTED_ANSI, example)));
    }
    out.push('\n');

    out.push_str(&format!("  {}\n", heading(color, "Global Options")));
    for (flag, desc) in GLOBAL_OPTIONS {
        push_row(&mut out, color, 2, 28, CYAN_ANSI, flag, desc);
    }
    out.push('\n');

    out.push_str(&format!("  {}\n", heading(color, "Environment")));
    for (name, desc) in ENVIRONMENT {
        push_row(&mut out, color, 2, 28, CYAN_ANSI, name, desc);
    }
    out.push('\n');

    out.push_str(&format!("  {}\n", heading(color, "Commands")));
    for (section, names) in SECTIONS {
        out.push_str(&format!("  {}\n", paint(color, CYAN_ANSI, section)));
        for name in *names {
            if let Some(doc) = lookup(name) {
                push_row(&mut out, color, 4, 18, PRIMARY_ANSI, doc.name, doc.summary);
            }
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "  {}\n",
        paint(
            color,
            MUTED_ANSI,
            "→ Run cortex <command> --help for command-specific flags"
        )
    ));
    out
}

/// Render per-command help, or `None` if the command is unknown.
pub(crate) fn render_command(name: &str, color: bool) -> Option<String> {
    if let Some(doc) = nested_lookup(name) {
        let mut out = String::with_capacity(512);
        out.push_str(&format!(
            "  {}  {}\n\n",
            heading(color, doc.path),
            paint(color, MUTED_ANSI, doc.summary)
        ));
        out.push_str(&format!("  {}\n", heading(color, "Usage")));
        for line in doc.usage {
            out.push_str(&format!("  {}\n", paint(color, CYAN_ANSI, line)));
        }
        return Some(out);
    }
    let doc = lookup(name)?;
    let mut out = String::with_capacity(512);
    out.push_str(&format!(
        "  {}  {}\n\n",
        heading(color, doc.name),
        paint(color, MUTED_ANSI, doc.summary)
    ));
    out.push_str(&format!("  {}\n", heading(color, "Usage")));
    for line in doc.usage {
        out.push_str(&format!("  {}\n", paint(color, CYAN_ANSI, line)));
    }
    Some(out)
}

fn lookup(name: &str) -> Option<&'static CommandDoc> {
    CATALOG.iter().find(|d| d.name == name)
}

fn nested_lookup(path: &str) -> Option<&'static NestedCommandDoc> {
    NESTED_CATALOG.iter().find(|d| d.path == path)
}

fn is_known(name: &str) -> bool {
    lookup(name).is_some()
}

// ── Help-request interception ───────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HelpRequest {
    TopLevel,
    Command(String),
    None,
}

/// Classify a help request from the argv tail (program name already removed,
/// `--color`/`--no-color` already stripped). Stops at a `--` sentinel so
/// wrapped commands are untouched.
///
/// Help is recognized **positionally** (matching axon): the `--help`/`-h`
/// *flags* count anywhere (nobody queries those literals), but the bare word
/// `help` counts only in command position (`args[0]`). This keeps free-text
/// queries that contain "help" working — `cortex search help`,
/// `cortex ai search help`, and `cortex ai abuse --term help` all run the
/// query, not the help banner.
pub(crate) fn classify_help(args: &[String]) -> HelpRequest {
    let is_help_flag = |s: &str| matches!(s, "-h" | "--help");
    let scan: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .take_while(|a| *a != "--")
        .collect();
    if scan.is_empty() {
        return HelpRequest::None;
    }
    let has_help = scan.iter().any(|a| is_help_flag(a)) || scan.first() == Some(&"help");
    if !has_help {
        return HelpRequest::None;
    }
    // Build the command path, skipping the value consumed by a value-bearing
    // global option (`--server URL`, `--token TOK`). Otherwise
    // `cortex --server http://127.0.0.1:3100 db status --help` would treat the
    // URL as the command and fall back to the top-level banner instead of
    // resolving `db status`. (The `--flag=value` form is one `-`-prefixed token
    // and is already excluded.)
    const VALUE_FLAGS: [&str; 2] = ["--server", "--token"];
    let mut positionals = Vec::new();
    let mut skip_value = false;
    for &a in &scan {
        if skip_value {
            skip_value = false;
            continue;
        }
        if VALUE_FLAGS.contains(&a) {
            skip_value = true;
            continue;
        }
        if !a.starts_with('-') && a != "help" {
            positionals.push(a);
        }
    }
    if positionals.len() >= 2 {
        let nested = format!("{} {}", positionals[0], positionals[1]);
        if nested_lookup(&nested).is_some() {
            return HelpRequest::Command(nested);
        }
    }
    match positionals.first().copied() {
        Some(cmd) if is_known(cmd) => HelpRequest::Command(cmd.to_string()),
        _ => HelpRequest::TopLevel,
    }
}

/// Handle an explicit help request by printing to stdout and returning true
/// (caller should exit 0). Returns false when no help was requested.
pub(crate) fn maybe_handle_help(args: &[String]) -> bool {
    match classify_help(args) {
        HelpRequest::TopLevel => {
            print!("{}", render_top_level(color::color_enabled()));
            true
        }
        HelpRequest::Command(name) => {
            // Known by construction (classify only returns Command for known).
            if let Some(text) = render_command(&name, color::color_enabled()) {
                print!("{text}");
            }
            true
        }
        HelpRequest::None => false,
    }
}

/// Top-level help to stderr — used by error/misuse fallbacks (`print_usage`).
pub(crate) fn print_top_level_help_stderr() {
    eprint!("{}", render_top_level(color::color_enabled_stderr()));
}

#[cfg(test)]
#[path = "help_tests.rs"]
mod tests;
