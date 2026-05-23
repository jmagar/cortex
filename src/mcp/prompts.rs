use rmcp::model::{Prompt, PromptArgument, PromptMessage, PromptMessageRole};
use serde_json::Map;

#[derive(Clone, Copy)]
struct PromptSpec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    arguments: &'static [PromptArgSpec],
    body: fn(&Map<String, serde_json::Value>) -> String,
}

#[derive(Clone, Copy)]
struct PromptArgSpec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    required: bool,
}

const WINDOW_ARG: PromptArgSpec = PromptArgSpec {
    name: "window",
    title: "Window",
    description: "Investigation window, for example 'last 30 minutes' or an RFC3339 range.",
    required: false,
};

const HOST_ARG: PromptArgSpec = PromptArgSpec {
    name: "host",
    title: "Host",
    description: "Hostname or source identifier to focus on.",
    required: false,
};

const SERVICE_ARG: PromptArgSpec = PromptArgSpec {
    name: "service",
    title: "Service",
    description: "Service, application, container, or syslog app name to investigate.",
    required: false,
};

const PROMPTS: &[PromptSpec] = &[
    PromptSpec {
        name: "infra.incident-triage",
        title: "Incident Triage",
        description: "Build a timeline, scope, and next actions for a suspected infrastructure incident.",
        arguments: &[WINDOW_ARG, HOST_ARG, SERVICE_ARG],
        body: incident_triage_prompt,
    },
    PromptSpec {
        name: "infra.host-health",
        title: "Host Health Check",
        description: "Investigate whether one host is silent, noisy, clock-skewed, or producing errors.",
        arguments: &[
            PromptArgSpec {
                required: true,
                ..HOST_ARG
            },
            WINDOW_ARG,
        ],
        body: host_health_prompt,
    },
    PromptSpec {
        name: "infra.service-outage",
        title: "Service Outage",
        description: "Debug a service, app, or container outage from logs and correlated host events.",
        arguments: &[
            PromptArgSpec {
                required: true,
                ..SERVICE_ARG
            },
            HOST_ARG,
            WINDOW_ARG,
        ],
        body: service_outage_prompt,
    },
    PromptSpec {
        name: "infra.security-auth-review",
        title: "Security Auth Review",
        description: "Review authentication failures, bans, suspicious IPs, and related infrastructure context.",
        arguments: &[
            WINDOW_ARG,
            PromptArgSpec {
                name: "actor",
                title: "Actor",
                description: "Optional username, email, token subject, or client IP to focus on.",
                required: false,
            },
            HOST_ARG,
        ],
        body: security_auth_review_prompt,
    },
    PromptSpec {
        name: "infra.noise-reduction",
        title: "Noise Reduction",
        description: "Find repeated log patterns and propose safe alerting or suppression changes.",
        arguments: &[WINDOW_ARG, HOST_ARG, SERVICE_ARG],
        body: noise_reduction_prompt,
    },
    PromptSpec {
        name: "infra.agent-change-correlation",
        title: "Agent Change Correlation",
        description: "Correlate AI agent activity with infrastructure errors and regressions.",
        arguments: &[
            PromptArgSpec {
                name: "project",
                title: "Project",
                description: "Exact AI project path to inspect, if known.",
                required: false,
            },
            PromptArgSpec {
                name: "session_id",
                title: "Session ID",
                description: "AI transcript session id to focus on, if known.",
                required: false,
            },
            WINDOW_ARG,
            HOST_ARG,
            SERVICE_ARG,
        ],
        body: agent_change_correlation_prompt,
    },
];

pub(super) fn prompt_definitions() -> Vec<Prompt> {
    PROMPTS
        .iter()
        .map(|spec| {
            Prompt::new(
                spec.name,
                Some(spec.description),
                Some(
                    spec.arguments
                        .iter()
                        .map(|arg| {
                            PromptArgument::new(arg.name)
                                .with_title(arg.title)
                                .with_description(arg.description)
                                .with_required(arg.required)
                        })
                        .collect(),
                ),
            )
            .with_title(spec.title)
        })
        .collect()
}

pub(super) fn get_prompt(
    name: &str,
    arguments: Option<&Map<String, serde_json::Value>>,
) -> Option<(String, Vec<PromptMessage>)> {
    let spec = PROMPTS.iter().find(|spec| spec.name == name)?;
    let empty = Map::new();
    let args = arguments.unwrap_or(&empty);
    let text = (spec.body)(args);
    Some((
        spec.description.to_string(),
        vec![PromptMessage::new_text(PromptMessageRole::User, text)],
    ))
}

fn arg(args: &Map<String, serde_json::Value>, name: &str, fallback: &str) -> String {
    args.get(name)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn incident_triage_prompt(args: &Map<String, serde_json::Value>) -> String {
    let window = arg(args, "window", "the suspected incident window");
    let host = arg(args, "host", "all hosts");
    let service = arg(args, "service", "all relevant services");
    format!(
        r#"Investigate an infrastructure incident in syslog-mcp.

Scope:
- Time window: {window}
- Host focus: {host}
- Service focus: {service}

Use the `syslog` MCP tool to gather evidence before drawing conclusions:
1. Start with `action=status` and `action=stats` to confirm the database and ingest path are healthy.
2. Use `action=errors`, `action=timeline`, and `action=anomalies` for the requested window to identify error spikes and affected hosts.
3. Use `action=correlate` around the first suspicious timestamp, filtered by host/service if supplied.
4. Use `action=search` for exact error terms, then `action=context` or `action=get` for surrounding events.
5. Check `action=silent_hosts`, `action=clock_skew`, and `action=ingest_rate` if missing telemetry or bad timestamps could explain the symptoms.

Return:
- Incident summary with confidence level.
- Timeline of key events with timestamps, host, app, severity, and log ids when available.
- Likely root cause versus alternatives still supported by evidence.
- Concrete next commands or remediation steps.
- Gaps in telemetry or follow-up queries needed."#
    )
}

fn host_health_prompt(args: &Map<String, serde_json::Value>) -> String {
    let host = arg(args, "host", "the target host");
    let window = arg(args, "window", "the last 24 hours");
    format!(
        r#"Assess health for host `{host}` using syslog-mcp.

Use the `syslog` MCP tool:
1. `action=hosts` to confirm the host exists and inspect first/last seen.
2. `action=tail` with `hostname={host}` for the latest events.
3. `action=errors`, `action=timeline`, and `action=patterns` scoped to `{host}` over {window}.
4. `action=clock_skew` and `action=silent_hosts` to catch time drift or missing forwarding.
5. `action=source_ips` if hostname/source identity looks inconsistent.

Return a concise health verdict, top error patterns, whether forwarding is current, and the safest next operational checks."#
    )
}

fn service_outage_prompt(args: &Map<String, serde_json::Value>) -> String {
    let service = arg(args, "service", "the affected service");
    let host = arg(args, "host", "all likely hosts");
    let window = arg(args, "window", "the outage window");
    format!(
        r#"Debug a possible outage for service `{service}`.

Scope:
- Host: {host}
- Window: {window}

Use the `syslog` MCP tool:
1. Search service-specific logs with `action=search` using `app_name`, `hostname`, and exact error terms where possible.
2. Use `action=errors`, `action=timeline`, and `action=anomalies` to compare the outage window against baseline behavior.
3. Use `action=correlate` around the earliest failure to find host, Docker, network, DNS, auth, or proxy events nearby.
4. Use `action=context` around representative log ids instead of relying on isolated lines.
5. If this is syslog-mcp itself, also inspect `action=compose_status` and `action=compose_doctor`.

Return the likely failure mode, earliest visible symptom, blast radius, supporting log ids, and a ranked remediation checklist."#
    )
}

fn security_auth_review_prompt(args: &Map<String, serde_json::Value>) -> String {
    let window = arg(args, "window", "the review window");
    let actor = arg(
        args,
        "actor",
        "any suspicious user, token subject, or client IP",
    );
    let host = arg(args, "host", "all authentication-related hosts");
    format!(
        r#"Review authentication and security-relevant syslog activity.

Scope:
- Window: {window}
- Actor/IP focus: {actor}
- Host focus: {host}

Use the `syslog` MCP tool:
1. Search for auth failures, bans, MFA/challenge events, proxy denials, and suspicious source IPs with `action=search`.
2. Use structured filters such as `auth_outcome` when available.
3. Use `action=timeline`, `action=patterns`, and `action=source_ips` to separate repeated noise from targeted activity.
4. Use `action=correlate` around suspicious bursts to find related service, firewall, DNS, or container events.
5. Use `action=context` for representative events before making a security claim.

Return confirmed evidence, suspicious-but-unconfirmed signals, affected accounts/IPs/hosts, immediate containment options, and false-positive considerations."#
    )
}

fn noise_reduction_prompt(args: &Map<String, serde_json::Value>) -> String {
    let window = arg(args, "window", "the current noisy period");
    let host = arg(args, "host", "all hosts");
    let service = arg(args, "service", "all services");
    format!(
        r#"Find noisy log patterns that are safe candidates for alert tuning.

Scope:
- Window: {window}
- Host: {host}
- Service: {service}

Use the `syslog` MCP tool:
1. `action=patterns` to identify repeated templates and representative messages.
2. `action=errors` and `action=timeline` to rank noise by severity and volume.
3. `action=search` and `action=context` for the top patterns so you do not suppress a real incident.
4. `action=compare` if there is a known normal window to separate regressions from background noise.

Return a ranked table with pattern, affected hosts/apps, volume, risk of suppressing it, and recommended action: fix source, lower severity, deduplicate, alert only on rate, or leave untouched."#
    )
}

fn agent_change_correlation_prompt(args: &Map<String, serde_json::Value>) -> String {
    let project = arg(args, "project", "the relevant project path if known");
    let session_id = arg(args, "session_id", "the relevant AI session if known");
    let window = arg(args, "window", "the suspected change window");
    let host = arg(args, "host", "affected hosts");
    let service = arg(args, "service", "affected services");
    format!(
        r#"Correlate AI agent activity with infrastructure symptoms.

Scope:
- Project: {project}
- Session: {session_id}
- Window: {window}
- Host: {host}
- Service: {service}

Use the `syslog` MCP tool:
1. `action=usage_blocks`, `action=sessions`, and `action=project_context` to identify relevant AI work.
2. `action=search_sessions` for deployment, compose, config, migration, auth, or service names.
3. `action=ai_correlate` to align AI transcript anchors with infrastructure logs.
4. `action=errors`, `action=timeline`, and `action=correlate` for infrastructure evidence near the same timestamps.

Return what changed, whether symptoms started after that change, supporting transcript/log evidence, alternative causes, and a rollback or verification plan."#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_catalog_names_are_unique() {
        let mut names: Vec<_> = PROMPTS.iter().map(|prompt| prompt.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PROMPTS.len());
    }

    #[test]
    fn rendered_prompts_reference_syslog_tool_actions() {
        for spec in PROMPTS {
            let (_description, messages) = get_prompt(spec.name, None).unwrap();
            let text = match &messages[0].content {
                rmcp::model::PromptMessageContent::Text { text } => text,
                _ => panic!("expected text prompt"),
            };
            assert!(text.contains("syslog"));
            assert!(text.contains("action="));
        }
    }
}
