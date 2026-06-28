//! Executable command/API surface matrix.
//!
//! This is the control-plane registry for the public surface consolidation
//! work. It records which spellings are canonical today, which old spellings
//! are intentional clean breaks, and which routes are intentionally removed.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceClass {
    Canonical,
    Operational,
    Hidden,
    RemovedCleanBreak,
    OutOfScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceDomain {
    Logs,
    Hosts,
    Sessions,
    Analysis,
    Correlate,
    State,
    Stats,
    Ingest,
    Alerts,
    Graph,
    Runtime,
    Setup,
    Database,
    Config,
}

#[derive(Debug, Clone, Copy)]
pub struct CliSurface {
    pub name: &'static str,
    pub class: SurfaceClass,
    pub domain: SurfaceDomain,
    pub local_cli: bool,
    pub http_cli: bool,
    pub replacement: Option<&'static str>,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ApiSurface {
    pub path: &'static str,
    pub class: SurfaceClass,
    pub domain: SurfaceDomain,
    pub replacement: Option<&'static str>,
    pub reason: &'static str,
}

pub const TOP_LEVEL_COMMANDS: &[&str] = &[
    "search",
    "filter",
    "tail",
    "errors",
    "hosts",
    "sessions",
    "incident",
    "heartbeat",
    "correlate",
    "state",
    "ingest",
    "stats",
    "compose",
    "setup",
    "db",
    "config",
    "timeline",
    "patterns",
    "alerts",
    "anomalies",
    "compare",
    "apps",
    "correlate-state",
    "topic-correlate",
    "entity",
    "graph",
    "completions",
];

pub const CLI_MODE_COMMANDS: &[&str] = &[
    "search",
    "filter",
    "tail",
    "errors",
    "hosts",
    "sessions",
    "incident",
    "entity",
    "graph",
    "heartbeat",
    "correlate",
    "state",
    "ingest",
    "stats",
    "db",
    "compose",
    "setup",
    "config",
    "timeline",
    "patterns",
    "alerts",
    "anomalies",
    "compare",
    "apps",
    "correlate-state",
    "topic-correlate",
    "__complete",
    "completions",
];

pub const CLI_SURFACES: &[CliSurface] = &[
    cli("search", SurfaceDomain::Logs, true),
    cli("filter", SurfaceDomain::Logs, true),
    cli("tail", SurfaceDomain::Logs, true),
    cli("errors", SurfaceDomain::Logs, true),
    cli("hosts", SurfaceDomain::Hosts, true),
    cli("sessions", SurfaceDomain::Sessions, true),
    cli("incident", SurfaceDomain::Analysis, true),
    cli("correlate", SurfaceDomain::Correlate, true),
    cli("state", SurfaceDomain::State, true),
    cli("ingest", SurfaceDomain::Ingest, true),
    cli("stats", SurfaceDomain::Stats, true),
    cli("timeline", SurfaceDomain::Analysis, true),
    cli("patterns", SurfaceDomain::Analysis, true),
    cli("anomalies", SurfaceDomain::Analysis, true),
    cli("compare", SurfaceDomain::Analysis, true),
    cli("apps", SurfaceDomain::Logs, true),
    cli("correlate-state", SurfaceDomain::Correlate, true),
    cli("topic-correlate", SurfaceDomain::Correlate, true),
    cli("entity", SurfaceDomain::Graph, true),
    cli("graph", SurfaceDomain::Graph, true),
    operational("heartbeat", SurfaceDomain::Ingest, false),
    cli("alerts", SurfaceDomain::Alerts, true),
    operational("compose", SurfaceDomain::Runtime, false),
    operational("setup", SurfaceDomain::Setup, false),
    operational("db", SurfaceDomain::Database, false),
    operational("config", SurfaceDomain::Config, false),
    hidden("__complete", SurfaceDomain::Config),
    hidden("completions", SurfaceDomain::Config),
    removed(
        "ai",
        SurfaceDomain::Sessions,
        "sessions",
        "AI transcript operations moved under sessions",
    ),
    removed(
        "source-ips",
        SurfaceDomain::Hosts,
        "hosts sources",
        "source identities moved under hosts",
    ),
    removed(
        "silent-hosts",
        SurfaceDomain::Hosts,
        "hosts silent",
        "silent host detection moved under hosts",
    ),
    removed(
        "service",
        SurfaceDomain::Runtime,
        "compose logs SERVICE",
        "container service logs moved under compose logs",
    ),
    removed(
        "deploy",
        SurfaceDomain::Setup,
        "setup deploy",
        "deployment workflows moved under setup deploy",
    ),
    removed(
        "sig",
        SurfaceDomain::Alerts,
        "alerts signatures",
        "error-signature operations moved under alerts signatures",
    ),
    removed(
        "notify",
        SurfaceDomain::Alerts,
        "alerts notifications",
        "notification operations moved under alerts notifications",
    ),
    removed(
        "host-state",
        SurfaceDomain::State,
        "state host",
        "host state moved under state",
    ),
    removed(
        "fleet-state",
        SurfaceDomain::State,
        "state fleet",
        "fleet state moved under state",
    ),
    removed(
        "clock-skew",
        SurfaceDomain::State,
        "state clock-skew",
        "clock skew moved under state",
    ),
    removed(
        "ingest-rate",
        SurfaceDomain::Stats,
        "stats ingest-rate",
        "ingest throughput moved under stats",
    ),
    removed(
        "shell",
        SurfaceDomain::Ingest,
        "ingest shell",
        "shell-history ingestion moved under ingest",
    ),
    removed(
        "agent-command",
        SurfaceDomain::Ingest,
        "ingest agent-command",
        "agent command ingestion moved under ingest",
    ),
    removed(
        "inventory",
        SurfaceDomain::Ingest,
        "ingest inventory",
        "inventory ingestion moved under ingest",
    ),
    removed(
        "file-tail",
        SurfaceDomain::Ingest,
        "ingest file-tail",
        "file-tail ingestion moved under ingest",
    ),
];

pub const API_SURFACES: &[ApiSurface] = &[
    api("/api/search", SurfaceDomain::Logs),
    api("/api/filter", SurfaceDomain::Logs),
    api("/api/tail", SurfaceDomain::Logs),
    api("/api/errors", SurfaceDomain::Logs),
    api("/api/hosts", SurfaceDomain::Hosts),
    api("/api/source-ips", SurfaceDomain::Hosts),
    api("/api/silent-hosts", SurfaceDomain::Hosts),
    api("/api/correlate", SurfaceDomain::Correlate),
    api("/api/correlate-state", SurfaceDomain::Correlate),
    api("/api/topic-correlate", SurfaceDomain::Correlate),
    api("/api/stats", SurfaceDomain::Stats),
    api("/api/ingest-rate", SurfaceDomain::Stats),
    api("/api/timeline", SurfaceDomain::Analysis),
    api("/api/patterns", SurfaceDomain::Analysis),
    api("/api/anomalies", SurfaceDomain::Analysis),
    api("/api/compare", SurfaceDomain::Analysis),
    api("/api/similar-incidents", SurfaceDomain::Analysis),
    api("/api/incident-context", SurfaceDomain::Analysis),
    api("/api/host-state", SurfaceDomain::State),
    api("/api/fleet-state", SurfaceDomain::State),
    api("/api/clock-skew", SurfaceDomain::State),
    api("/api/apps", SurfaceDomain::Logs),
    api("/api/context", SurfaceDomain::Logs),
    api("/api/get", SurfaceDomain::Logs),
    api("/api/version", SurfaceDomain::Runtime),
    api("/api/compose/status", SurfaceDomain::Runtime),
    api("/api/compose/doctor", SurfaceDomain::Runtime),
    api("/api/errors/unaddressed", SurfaceDomain::Alerts),
    api("/api/errors/ack", SurfaceDomain::Alerts),
    api("/api/errors/unack", SurfaceDomain::Alerts),
    api("/api/notifications/recent", SurfaceDomain::Alerts),
    api("/api/notifications/test", SurfaceDomain::Alerts),
    api("/api/file-tails", SurfaceDomain::Ingest),
    api("/api/db/status", SurfaceDomain::Database),
    api("/api/db/integrity", SurfaceDomain::Database),
    api("/api/db/integrity/background", SurfaceDomain::Database),
    api("/api/db/integrity/jobs/{id}", SurfaceDomain::Database),
    api("/api/db/checkpoint", SurfaceDomain::Database),
    api("/api/db/vacuum", SurfaceDomain::Database),
    api("/api/db/backup", SurfaceDomain::Database),
    api("/api/graph/entity", SurfaceDomain::Graph),
    api("/api/graph/around", SurfaceDomain::Graph),
    api("/api/graph/explain", SurfaceDomain::Graph),
    api("/api/graph/evidence", SurfaceDomain::Graph),
    api("/api/sessions", SurfaceDomain::Sessions),
    api("/api/sessions/search", SurfaceDomain::Sessions),
    api("/api/sessions/abuse", SurfaceDomain::Sessions),
    api("/api/sessions/correlate", SurfaceDomain::Sessions),
    api("/api/sessions/blocks", SurfaceDomain::Sessions),
    api("/api/sessions/context", SurfaceDomain::Sessions),
    api("/api/sessions/tools", SurfaceDomain::Sessions),
    api("/api/sessions/projects", SurfaceDomain::Sessions),
    api("/api/sessions/ask-history", SurfaceDomain::Sessions),
    api("/api/sessions/incidents", SurfaceDomain::Sessions),
    api("/api/sessions/investigate", SurfaceDomain::Sessions),
    removed_api(
        "/api/ai/*",
        SurfaceDomain::Sessions,
        "/api/sessions/*",
        "clean protocol break",
    ),
];

pub fn is_cli_mode_command(name: &str) -> bool {
    CLI_MODE_COMMANDS.contains(&name)
}

pub fn removed_cli_surface(name: &str) -> Option<&'static CliSurface> {
    CLI_SURFACES
        .iter()
        .find(|surface| surface.name == name && surface.class == SurfaceClass::RemovedCleanBreak)
}

const fn cli(name: &'static str, domain: SurfaceDomain, http_cli: bool) -> CliSurface {
    CliSurface {
        name,
        class: SurfaceClass::Canonical,
        domain,
        local_cli: true,
        http_cli,
        replacement: None,
        reason: "",
    }
}

const fn operational(name: &'static str, domain: SurfaceDomain, http_cli: bool) -> CliSurface {
    CliSurface {
        name,
        class: SurfaceClass::Operational,
        domain,
        local_cli: true,
        http_cli,
        replacement: None,
        reason: "",
    }
}

const fn hidden(name: &'static str, domain: SurfaceDomain) -> CliSurface {
    CliSurface {
        name,
        class: SurfaceClass::Hidden,
        domain,
        local_cli: true,
        http_cli: false,
        replacement: None,
        reason: "",
    }
}

const fn removed(
    name: &'static str,
    domain: SurfaceDomain,
    replacement: &'static str,
    reason: &'static str,
) -> CliSurface {
    CliSurface {
        name,
        class: SurfaceClass::RemovedCleanBreak,
        domain,
        local_cli: false,
        http_cli: false,
        replacement: Some(replacement),
        reason,
    }
}

const fn api(path: &'static str, domain: SurfaceDomain) -> ApiSurface {
    ApiSurface {
        path,
        class: SurfaceClass::Canonical,
        domain,
        replacement: None,
        reason: "",
    }
}

const fn removed_api(
    path: &'static str,
    domain: SurfaceDomain,
    replacement: &'static str,
    reason: &'static str,
) -> ApiSurface {
    ApiSurface {
        path,
        class: SurfaceClass::RemovedCleanBreak,
        domain,
        replacement: Some(replacement),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_commands_are_registered_surfaces() {
        for command in TOP_LEVEL_COMMANDS {
            assert!(
                CLI_SURFACES.iter().any(|surface| surface.name == *command
                    && !matches!(surface.class, SurfaceClass::RemovedCleanBreak)),
                "{command} is in TOP_LEVEL_COMMANDS but not CLI_SURFACES"
            );
        }
    }

    #[test]
    fn parser_mode_commands_are_registered_surfaces() {
        for command in CLI_MODE_COMMANDS {
            assert!(
                CLI_SURFACES.iter().any(|surface| surface.name == *command
                    && !matches!(surface.class, SurfaceClass::RemovedCleanBreak)),
                "{command} is accepted by Mode but not CLI_SURFACES"
            );
        }
    }

    #[test]
    fn clean_break_cli_surfaces_have_replacements() {
        for command in [
            "ai",
            "source-ips",
            "silent-hosts",
            "service",
            "deploy",
            "sig",
            "notify",
            "host-state",
            "fleet-state",
            "clock-skew",
            "ingest-rate",
            "shell",
            "agent-command",
            "inventory",
            "file-tail",
        ] {
            let surface = removed_cli_surface(command)
                .unwrap_or_else(|| panic!("{command} is missing removed surface metadata"));
            assert!(
                surface.replacement.is_some(),
                "{command} needs replacement guidance"
            );
            assert!(!surface.reason.is_empty(), "{command} needs a break reason");
        }
    }

    #[test]
    fn prior_consolidations_are_classified_in_matrix() {
        for (name, domain, class, local_cli, http_cli) in [
            (
                "sessions",
                SurfaceDomain::Sessions,
                SurfaceClass::Canonical,
                true,
                true,
            ),
            (
                "hosts",
                SurfaceDomain::Hosts,
                SurfaceClass::Canonical,
                true,
                true,
            ),
            (
                "compose",
                SurfaceDomain::Runtime,
                SurfaceClass::Operational,
                true,
                false,
            ),
            (
                "setup",
                SurfaceDomain::Setup,
                SurfaceClass::Operational,
                true,
                false,
            ),
        ] {
            let surface = CLI_SURFACES
                .iter()
                .find(|surface| surface.name == name)
                .unwrap_or_else(|| panic!("{name} missing from CLI_SURFACES"));
            assert_eq!(surface.domain, domain);
            assert_eq!(surface.class, class);
            assert_eq!(surface.local_cli, local_cli);
            assert_eq!(surface.http_cli, http_cli);
        }
    }

    #[test]
    fn api_ai_is_recorded_as_clean_break() {
        let api_ai = API_SURFACES
            .iter()
            .find(|surface| surface.path == "/api/ai/*")
            .expect("/api/ai/* clean break row");
        assert_eq!(api_ai.class, SurfaceClass::RemovedCleanBreak);
        assert_eq!(api_ai.replacement, Some("/api/sessions/*"));
    }

    #[test]
    fn api_matrix_covers_current_route_families() {
        for path in [
            "/api/search",
            "/api/source-ips",
            "/api/sessions/search",
            "/api/correlate-state",
            "/api/host-state",
            "/api/compose/status",
            "/api/errors/ack",
            "/api/file-tails",
            "/api/db/status",
            "/api/graph/entity",
        ] {
            assert!(
                API_SURFACES.iter().any(|surface| surface.path == path
                    && surface.class != SurfaceClass::RemovedCleanBreak),
                "{path} is an active route missing from API_SURFACES"
            );
        }
    }
}
