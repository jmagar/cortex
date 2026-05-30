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

const CONTAINER_ARG: PromptArgSpec = PromptArgSpec {
    name: "container",
    title: "Container",
    description: "Docker container name or container-like app name to investigate.",
    required: false,
};

const COMMON_INVESTIGATION_RULES: &str = r#"Rules:
- Start with cheap, narrow calls. Use `limit=5` or `limit=10` for `action=search`, `action=errors`, `action=tail`, `action=patterns`, and `action=correlate`.
- For `action=timeline`, use `bucket=minute` for recent windows. Valid bucket values are `minute`, `hour`, and `day`.
- Use `action=context` with small bounds such as `before=3` and `after=3` around representative log ids.
- Escalate to broad or slower actions only when the cheap pass leaves a specific question: `action=stats`, `action=anomalies`, `action=patterns`, `action=compare`, `action=clock_skew`, `action=ingest_rate`, or wide `action=correlate`.
- Summarize representative evidence. Do not paste full JSON payloads or unbounded result sets.
- If the client supports structured output, conform to the `syslog://schema/prompt-output` resource schema."#;

const SYNTHESIS_FORMAT: &str = r#"Return exactly these sections:
- Verdict:
- Evidence:
- Likely Cause:
- Not Supported:
- Next Actions:
- Telemetry Gaps:"#;

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
    PromptSpec {
        name: "infra.docker-container-regression",
        title: "Docker Container Regression",
        description: "Investigate a container restart, healthcheck, image, or Compose regression.",
        arguments: &[CONTAINER_ARG, HOST_ARG, SERVICE_ARG, WINDOW_ARG],
        body: docker_container_regression_prompt,
    },
    PromptSpec {
        name: "infra.network-dns-failure",
        title: "Network DNS Failure",
        description: "Debug DNS, proxy, firewall, and network reachability failures from logs.",
        arguments: &[HOST_ARG, SERVICE_ARG, WINDOW_ARG],
        body: network_dns_failure_prompt,
    },
    PromptSpec {
        name: "infra.storage-pressure",
        title: "Storage Pressure",
        description: "Investigate disk pressure, database growth, cleanup, and write-block risk.",
        arguments: &[HOST_ARG, SERVICE_ARG, WINDOW_ARG],
        body: storage_pressure_prompt,
    },
    PromptSpec {
        name: "infra.auth-bruteforce",
        title: "Auth Bruteforce",
        description: "Investigate repeated authentication failures, bans, and suspicious sources.",
        arguments: &[
            WINDOW_ARG,
            PromptArgSpec {
                name: "actor",
                title: "Actor",
                description: "Optional username, email, token subject, or client IP to focus on.",
                required: false,
            },
            HOST_ARG,
            SERVICE_ARG,
        ],
        body: auth_bruteforce_prompt,
    },
    PromptSpec {
        name: "infra.syslog-forwarding-gap",
        title: "Syslog Forwarding Gap",
        description: "Investigate missing, stale, spoofed, or delayed syslog forwarding.",
        arguments: &[HOST_ARG, WINDOW_ARG],
        body: syslog_forwarding_gap_prompt,
    },
    PromptSpec {
        name: "infra.after-deploy-check",
        title: "After Deploy Check",
        description: "Verify service health and regressions after a deployment or config change.",
        arguments: &[SERVICE_ARG, HOST_ARG, WINDOW_ARG],
        body: after_deploy_check_prompt,
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
        r#"Investigate an infrastructure incident in cortex.

Scope:
- Time window: {window}
- Host focus: {host}
- Service focus: {service}

Use the `syslog` MCP tool to gather evidence before drawing conclusions:
Cheap first pass:
1. Use `action=status` to confirm the server is answering.
2. Use `action=errors` with `limit=10` for the requested window, host, and service filters.
3. Use `action=timeline` with `bucket=minute` for the same scope to identify the first abnormal minute.
4. Use `action=search` with `limit=5` for exact error terms found in the first pass.
5. Use `action=context` with `before=3` and `after=3` around the most representative log ids.

Escalate only if needed:
1. Use `action=correlate` with `limit=20` around the earliest suspicious timestamp.
2. Use `action=anomalies` or `action=compare` when you need baseline context.
3. Use `action=silent_hosts`, `action=clock_skew`, `action=ingest_rate`, or `action=stats` only when missing telemetry, timestamp drift, ingestion lag, or storage health could explain the symptoms.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
    )
}

fn host_health_prompt(args: &Map<String, serde_json::Value>) -> String {
    let host = arg(args, "host", "the target host");
    let window = arg(args, "window", "the last 24 hours");
    format!(
        r#"Assess health for host `{host}` using cortex.

Use the `syslog` MCP tool:
Cheap first pass:
1. Use `action=hosts` to confirm the host exists and inspect first/last seen.
2. Use `action=tail` with `hostname={host}` and `limit=10` for the latest events.
3. Use `action=errors` with `hostname={host}` and `limit=10` over {window}.
4. Use `action=timeline` with `hostname={host}` and `bucket=minute` over {window}.
5. Use `action=context` with `before=3` and `after=3` around representative error log ids.

Escalate only if needed:
1. Use `action=silent_hosts` if forwarding freshness is unclear.
2. Use `action=clock_skew` if timestamps do not line up with received time.
3. Use `action=patterns` with `hostname={host}` and `limit=10` only after identifying noisy apps or repeated errors.
4. Use `action=source_ips` if hostname/source identity looks inconsistent.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
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
Cheap first pass:
1. Use `action=search` with `app_name={service}`, `hostname` if known, and `limit=5`.
2. Use `action=errors` with `limit=10` for the service and host scope.
3. Use `action=timeline` with `bucket=minute` for the outage window.
4. Use `action=context` with `before=3` and `after=3` around representative service failures.

Escalate only if needed:
1. Use `action=correlate` with `limit=20` around the earliest failure to find host, Docker, network, DNS, auth, or proxy events nearby.
2. Use `action=anomalies` or `action=compare` only when baseline behavior matters.
3. If this is cortex itself, inspect `action=compose_status` first and `action=compose_doctor` only if the status projection is inconclusive.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
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
Cheap first pass:
1. Search for auth failures, bans, MFA/challenge events, proxy denials, and suspicious source IPs with `action=search` and `limit=5`.
2. Use structured filters such as `auth_outcome` when available.
3. Use `action=errors` with `limit=10` and `action=timeline` with `bucket=minute` for suspicious bursts.
4. Use `action=context` with `before=3` and `after=3` for representative events before making a security claim.

Escalate only if needed:
1. Use `action=source_ips` when actor or IP identity needs validation.
2. Use `action=patterns` with `limit=10` to separate repeated noise from targeted activity.
3. Use `action=correlate` with `limit=20` around suspicious bursts to find related service, firewall, DNS, or container events.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
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
Cheap first pass:
1. Use `action=errors` with `limit=10` to rank noisy severity groups.
2. Use `action=timeline` with `bucket=minute` to distinguish bursts from steady background noise.
3. Use `action=search` with `limit=5` for the top repeated message terms.
4. Use `action=context` with `before=3` and `after=3` before recommending suppression.

Escalate only if needed:
1. Use `action=patterns` with `limit=10` to identify repeated templates and representative messages.
2. Use `action=compare` only when there is a known normal window to separate regressions from background noise.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
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
Cheap first pass:
1. Use `action=usage_blocks` and `action=sessions` with `limit=10` to identify relevant AI work.
2. Use `action=search_sessions` with `limit=5` for deployment, compose, config, migration, auth, or service names.
3. Use `action=errors` with `limit=10` and `action=timeline` with `bucket=minute` for infrastructure evidence near the same timestamps.

Escalate only if needed:
1. Use `action=project_context` only for the most relevant project or session.
2. Use `action=ai_correlate` to align AI transcript anchors with infrastructure logs after you have at least one candidate timestamp.
3. Use `action=correlate` with `limit=20` around the candidate timestamp.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
    )
}

fn docker_container_regression_prompt(args: &Map<String, serde_json::Value>) -> String {
    let container = arg(args, "container", "the affected container");
    let host = arg(args, "host", "the Docker host");
    let service = arg(args, "service", "the affected service");
    let window = arg(args, "window", "the regression window");
    format!(
        r#"Investigate a Docker container regression with cortex.

Scope:
- Container: {container}
- Host: {host}
- Service: {service}
- Window: {window}

Use the `syslog` MCP tool:
Cheap first pass:
1. Use `action=search` with Docker/container terms and `limit=5`.
2. Use `action=errors` with `limit=10` for the host/service scope.
3. Use `action=timeline` with `bucket=minute` to find restart or error bursts.
4. Use `action=context` with `before=3` and `after=3` around representative Docker or service failures.

Escalate only if needed:
1. Use `action=compose_status` to inspect the canonical cortex deployment if the affected service is cortex.
2. Use `action=correlate` with `limit=20` around the first restart, healthcheck, pull, or OOM event.
3. Use `action=patterns` with `limit=10` only after identifying repeated container messages.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
    )
}

fn network_dns_failure_prompt(args: &Map<String, serde_json::Value>) -> String {
    let host = arg(args, "host", "the affected host or client");
    let service = arg(args, "service", "the affected service or upstream");
    let window = arg(args, "window", "the network failure window");
    format!(
        r#"Investigate a network or DNS failure with cortex.

Scope:
- Host/client: {host}
- Service/upstream: {service}
- Window: {window}

Use the `syslog` MCP tool:
Cheap first pass:
1. Use `action=search` with DNS, resolver, proxy, upstream, timeout, refused, and TLS terms plus `limit=5`.
2. Use `action=errors` with `limit=10` for affected hosts or apps.
3. Use `action=timeline` with `bucket=minute` to identify the first failed minute.
4. Use `action=context` with `before=3` and `after=3` around representative failures.

Escalate only if needed:
1. Use `action=source_ips` if source identity, NAT, or spoofing could affect attribution.
2. Use `action=correlate` with `limit=20` around the first network failure.
3. Use `action=compare` only when a known-good window is available.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
    )
}

fn storage_pressure_prompt(args: &Map<String, serde_json::Value>) -> String {
    let host = arg(args, "host", "the storage host");
    let service = arg(args, "service", "the service writing data");
    let window = arg(args, "window", "the storage pressure window");
    format!(
        r#"Investigate storage pressure with cortex.

Scope:
- Host: {host}
- Service: {service}
- Window: {window}

Use the `syslog` MCP tool:
Cheap first pass:
1. Use `action=status` to check current ingest/write-block state.
2. Use `action=search` with disk, space, quota, sqlite, wal, write, readonly, and database terms plus `limit=5`.
3. Use `action=errors` with `limit=10` for the storage host or service.
4. Use `action=timeline` with `bucket=minute` for storage-related errors.

Escalate only if needed:
1. Use `action=stats` only after the first pass suggests DB size, free disk, or write-block risk.
2. Use `action=context` with `before=3` and `after=3` around representative storage errors.
3. Use `action=correlate` with `limit=20` around the first storage warning.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
    )
}

fn auth_bruteforce_prompt(args: &Map<String, serde_json::Value>) -> String {
    let window = arg(args, "window", "the suspected brute-force window");
    let actor = arg(args, "actor", "the suspicious actor or source IP");
    let host = arg(args, "host", "authentication hosts");
    let service = arg(args, "service", "authentication service");
    format!(
        r#"Investigate possible authentication brute force with cortex.

Scope:
- Window: {window}
- Actor/IP: {actor}
- Host: {host}
- Service: {service}

Use the `syslog` MCP tool:
Cheap first pass:
1. Use `action=search` with auth failure, invalid user, banned, MFA, challenge, and denied terms plus `limit=5`.
2. Use `action=errors` with `limit=10` scoped to auth services and hosts.
3. Use `action=timeline` with `bucket=minute` to find bursts.
4. Use `action=context` with `before=3` and `after=3` around representative auth failures.

Escalate only if needed:
1. Use `action=source_ips` when source identity matters.
2. Use `action=patterns` with `limit=10` to separate repeated scanner noise from account-targeted activity.
3. Use `action=correlate` with `limit=20` around suspicious bursts to find firewall, proxy, or service-side effects.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
    )
}

fn syslog_forwarding_gap_prompt(args: &Map<String, serde_json::Value>) -> String {
    let host = arg(args, "host", "the host or device expected to forward logs");
    let window = arg(args, "window", "the suspected forwarding gap window");
    format!(
        r#"Investigate a syslog forwarding gap with cortex.

Scope:
- Host/device: {host}
- Window: {window}

Use the `syslog` MCP tool:
Cheap first pass:
1. Use `action=hosts` to inspect first_seen, last_seen, and total log count.
2. Use `action=tail` with `hostname={host}` and `limit=10`.
3. Use `action=timeline` with `hostname={host}` and `bucket=minute` for the expected window.
4. Use `action=search` with forwarding, rsyslog, syslog, dropped, refused, and queue terms plus `limit=5`.

Escalate only if needed:
1. Use `action=silent_hosts` if last_seen is stale.
2. Use `action=source_ips` if hostname spoofing or source identity drift is possible.
3. Use `action=clock_skew` only if timestamps are inconsistent with received_at.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
    )
}

fn after_deploy_check_prompt(args: &Map<String, serde_json::Value>) -> String {
    let service = arg(args, "service", "the deployed service");
    let host = arg(args, "host", "the deployment host");
    let window = arg(args, "window", "the post-deploy window");
    format!(
        r#"Run an after-deploy check with cortex.

Scope:
- Service: {service}
- Host: {host}
- Window: {window}

Use the `syslog` MCP tool:
Cheap first pass:
1. Use `action=status` to verify cortex is answering.
2. Use `action=search` with service, deploy, migration, restart, healthcheck, error, and warning terms plus `limit=5`.
3. Use `action=errors` with `limit=10` for the deployed service and host.
4. Use `action=timeline` with `bucket=minute` to compare pre/post deploy activity.
5. Use `action=context` with `before=3` and `after=3` around the first post-deploy error.

Escalate only if needed:
1. Use `action=compare` when you have a known pre-deploy window.
2. Use `action=correlate` with `limit=20` around the deploy timestamp.
3. Use `action=compose_status` or `action=compose_doctor` only for cortex deployment health.

{COMMON_INVESTIGATION_RULES}

{SYNTHESIS_FORMAT}"#
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

    #[test]
    fn rendered_prompts_reference_only_known_actions() {
        let known_actions = super::super::actions::action_names();
        for (name, text) in rendered_prompt_texts() {
            for action in action_references(&text) {
                assert!(
                    known_actions.contains(&action.as_str()),
                    "{name} references unknown action={action}"
                );
            }
        }
    }

    #[test]
    fn rendered_prompts_use_valid_timeline_bucket_guidance() {
        for (name, text) in rendered_prompt_texts() {
            assert!(
                !text.contains("bucket=5m") && !text.contains("`5m`") && !text.contains(" 5m"),
                "{name} includes invalid timeline bucket guidance"
            );

            if text.contains("action=timeline") {
                assert!(
                    text.contains("bucket=minute"),
                    "{name} mentions timeline without minute bucket guidance"
                );
                assert!(
                    text.contains("`minute`, `hour`, and `day`"),
                    "{name} does not spell out valid timeline buckets"
                );
            }
        }
    }

    #[test]
    fn rendered_prompts_require_bounded_queries_and_common_synthesis() {
        let expected_sections = [
            "- Verdict:",
            "- Evidence:",
            "- Likely Cause:",
            "- Not Supported:",
            "- Next Actions:",
            "- Telemetry Gaps:",
        ];

        for (name, text) in rendered_prompt_texts() {
            assert!(
                text.contains("limit=5"),
                "{name} lacks small search guidance"
            );
            assert!(
                text.contains("limit=10"),
                "{name} lacks bounded query guidance"
            );
            assert!(
                text.contains("before=3") && text.contains("after=3"),
                "{name} lacks bounded context guidance"
            );
            assert!(
                text.contains("Escalate only if needed"),
                "{name} lacks cheap-first escalation guidance"
            );
            assert!(
                text.contains("Cheap first pass:"),
                "{name} lacks cheap-first pass guidance"
            );

            for section in expected_sections {
                assert!(text.contains(section), "{name} lacks section {section}");
            }
        }
    }

    fn rendered_prompt_texts() -> Vec<(&'static str, String)> {
        PROMPTS
            .iter()
            .map(|spec| {
                let (_description, messages) = get_prompt(spec.name, None).unwrap();
                let text = match &messages[0].content {
                    rmcp::model::PromptMessageContent::Text { text } => text.clone(),
                    _ => panic!("expected text prompt"),
                };
                (spec.name, text)
            })
            .collect()
    }

    fn action_references(text: &str) -> Vec<String> {
        let mut actions = Vec::new();
        let mut rest = text;

        while let Some(index) = rest.find("action=") {
            let after_marker = &rest[index + "action=".len()..];
            let action: String = after_marker
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .collect();
            if !action.is_empty() {
                actions.push(action);
            }
            rest = after_marker;
        }

        actions
    }
}
