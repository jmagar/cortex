use anyhow::{Result, bail};

use super::parse_common::{FlagCursor, parse_output_args, parse_u32_flag, value_after_equals};
use super::timearg::parse_time_arg;
use super::{
    CliCommand, CorrelateArgs, FilterArgs, IncidentArgs, IngestRateArgs, PatternsArgs, SearchArgs,
    SessionsArgs, SourceIpsArgs, TailArgs, TimeRangeArgs, TimelineArgs,
};

/// Normalize a user time value (relative or absolute) to RFC3339 at parse time.
fn norm_time(raw: String) -> Result<String> {
    parse_time_arg(&raw, chrono::Utc::now())
}

pub(crate) fn parse_search(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SearchArgs::default();
    let mut query = Vec::new();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--source-ip" => parsed.source_ip = Some(flags.value("--source-ip")?),
            "--severity" => parsed.severity = Some(flags.value("--severity")?),
            "--app-name" => parsed.app_name = Some(flags.value("--app-name")?),
            "--facility" => parsed.facility = Some(flags.value("--facility")?),
            "--exclude-facility" => {
                parsed.exclude_facility = Some(flags.value("--exclude-facility")?)
            }
            "--from" => parsed.from = Some(norm_time(flags.value("--from")?)?),
            "--to" => parsed.to = Some(norm_time(flags.value("--to")?)?),
            "--received-from" => {
                parsed.received_from = Some(norm_time(flags.value("--received-from")?)?)
            }
            "--received-to" => parsed.received_to = Some(norm_time(flags.value("--received-to")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "-h" | "--help" => bail!("use `cortex --help` for usage"),
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--source-ip=") => {
                parsed.source_ip = Some(value_after_equals(arg, "--source-ip")?)
            }
            _ if arg.starts_with("--severity=") => {
                parsed.severity = Some(value_after_equals(arg, "--severity")?)
            }
            _ if arg.starts_with("--app-name=") => {
                parsed.app_name = Some(value_after_equals(arg, "--app-name")?)
            }
            _ if arg.starts_with("--facility=") => {
                parsed.facility = Some(value_after_equals(arg, "--facility")?)
            }
            _ if arg.starts_with("--exclude-facility=") => {
                parsed.exclude_facility = Some(value_after_equals(arg, "--exclude-facility")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(norm_time(value_after_equals(arg, "--from")?)?)
            }
            _ if arg.starts_with("--to=") => {
                parsed.to = Some(norm_time(value_after_equals(arg, "--to")?)?)
            }
            _ if arg.starts_with("--received-from=") => {
                parsed.received_from = Some(norm_time(value_after_equals(arg, "--received-from")?)?)
            }
            _ if arg.starts_with("--received-to=") => {
                parsed.received_to = Some(norm_time(value_after_equals(arg, "--received-to")?)?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            "--grep" => parsed.grep = Some(flags.value("--grep")?),
            _ if arg.starts_with("--grep=") => {
                parsed.grep = Some(value_after_equals(arg, "--grep")?)
            }
            _ if arg.starts_with('-') => bail!("unknown search option: {arg}"),
            _ => query.push(arg),
        }
    }
    parsed.query = (!query.is_empty()).then(|| query.join(" "));
    if parsed.grep.is_some() && parsed.query.is_some() {
        bail!("--grep and a positional query are mutually exclusive; use one or the other");
    }
    if parsed.grep.as_deref().is_some_and(|g| g.trim().is_empty()) {
        bail!("--grep requires non-empty text");
    }
    Ok(CliCommand::Search(parsed))
}

pub(crate) fn parse_filter(args: &[String]) -> Result<CliCommand> {
    let mut parsed = FilterArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--source-ip" => parsed.source_ip = Some(flags.value("--source-ip")?),
            "--severity" => parsed.severity = Some(flags.value("--severity")?),
            "--app-name" => parsed.app_name = Some(flags.value("--app-name")?),
            "--facility" => parsed.facility = Some(flags.value("--facility")?),
            "--exclude-facility" => {
                parsed.exclude_facility = Some(flags.value("--exclude-facility")?)
            }
            "--process-id" => parsed.process_id = Some(flags.value("--process-id")?),
            "--from" => parsed.from = Some(norm_time(flags.value("--from")?)?),
            "--to" => parsed.to = Some(norm_time(flags.value("--to")?)?),
            "--received-from" => {
                parsed.received_from = Some(norm_time(flags.value("--received-from")?)?)
            }
            "--received-to" => parsed.received_to = Some(norm_time(flags.value("--received-to")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            "--source-kind" => parsed.source_kind = Some(flags.value("--source-kind")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--session-id" => parsed.session_id = Some(flags.value("--session-id")?),
            "--container" => parsed.container = Some(flags.value("--container")?),
            "--docker-host" => parsed.docker_host = Some(flags.value("--docker-host")?),
            "--stream" => parsed.stream = Some(flags.value("--stream")?),
            "--event-action" => parsed.event_action = Some(flags.value("--event-action")?),
            "-h" | "--help" => bail!("use `cortex --help` for usage"),
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--source-ip=") => {
                parsed.source_ip = Some(value_after_equals(arg, "--source-ip")?)
            }
            _ if arg.starts_with("--severity=") => {
                parsed.severity = Some(value_after_equals(arg, "--severity")?)
            }
            _ if arg.starts_with("--app-name=") => {
                parsed.app_name = Some(value_after_equals(arg, "--app-name")?)
            }
            _ if arg.starts_with("--facility=") => {
                parsed.facility = Some(value_after_equals(arg, "--facility")?)
            }
            _ if arg.starts_with("--exclude-facility=") => {
                parsed.exclude_facility = Some(value_after_equals(arg, "--exclude-facility")?)
            }
            _ if arg.starts_with("--process-id=") => {
                parsed.process_id = Some(value_after_equals(arg, "--process-id")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(norm_time(value_after_equals(arg, "--from")?)?)
            }
            _ if arg.starts_with("--to=") => {
                parsed.to = Some(norm_time(value_after_equals(arg, "--to")?)?)
            }
            _ if arg.starts_with("--received-from=") => {
                parsed.received_from = Some(norm_time(value_after_equals(arg, "--received-from")?)?)
            }
            _ if arg.starts_with("--received-to=") => {
                parsed.received_to = Some(norm_time(value_after_equals(arg, "--received-to")?)?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with("--source-kind=") => {
                parsed.source_kind = Some(value_after_equals(arg, "--source-kind")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--session-id=") => {
                parsed.session_id = Some(value_after_equals(arg, "--session-id")?)
            }
            _ if arg.starts_with("--container=") => {
                parsed.container = Some(value_after_equals(arg, "--container")?)
            }
            _ if arg.starts_with("--docker-host=") => {
                parsed.docker_host = Some(value_after_equals(arg, "--docker-host")?)
            }
            _ if arg.starts_with("--stream=") => {
                parsed.stream = Some(value_after_equals(arg, "--stream")?)
            }
            _ if arg.starts_with("--event-action=") => {
                parsed.event_action = Some(value_after_equals(arg, "--event-action")?)
            }
            _ if arg.starts_with('-') => bail!("unknown filter option: {arg}"),
            _ => {
                bail!("filter does not accept positional query terms; use `search` for FTS queries")
            }
        }
    }
    Ok(CliCommand::Filter(parsed))
}

pub(crate) fn parse_tail(args: &[String]) -> Result<CliCommand> {
    let mut parsed = TailArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--source-ip" => parsed.source_ip = Some(flags.value("--source-ip")?),
            "--app-name" => parsed.app_name = Some(flags.value("--app-name")?),
            "--n" | "-n" => parsed.n = Some(parse_u32_flag(&arg, flags.value(&arg)?)?),
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--source-ip=") => {
                parsed.source_ip = Some(value_after_equals(arg, "--source-ip")?)
            }
            _ if arg.starts_with("--app-name=") => {
                parsed.app_name = Some(value_after_equals(arg, "--app-name")?)
            }
            _ if arg.starts_with("--n=") => {
                parsed.n = Some(parse_u32_flag("--n", value_after_equals(arg, "--n")?)?)
            }
            _ if arg.starts_with('-') => bail!("unknown tail option: {arg}"),
            _ => parsed.n = Some(parse_u32_flag("n", arg)?),
        }
    }
    Ok(CliCommand::Tail(parsed))
}

pub(crate) fn parse_errors(args: &[String]) -> Result<CliCommand> {
    let mut parsed = TimeRangeArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--from" => parsed.from = Some(norm_time(flags.value("--from")?)?),
            "--to" => parsed.to = Some(norm_time(flags.value("--to")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(norm_time(value_after_equals(arg, "--from")?)?)
            }
            _ if arg.starts_with("--to=") => {
                parsed.to = Some(norm_time(value_after_equals(arg, "--to")?)?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ => bail!("unknown errors option: {arg}"),
        }
    }
    Ok(CliCommand::Errors(parsed))
}

pub(crate) fn parse_hosts(args: &[String]) -> Result<CliCommand> {
    Ok(CliCommand::Hosts(parse_output_args("hosts", args)?))
}

pub(crate) fn parse_sessions(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SessionsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--project" => parsed.project = Some(flags.value("--project")?),
            "--tool" => parsed.tool = Some(flags.value("--tool")?),
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--from" => parsed.from = Some(norm_time(flags.value("--from")?)?),
            "--to" => parsed.to = Some(norm_time(flags.value("--to")?)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--project=") => {
                parsed.project = Some(value_after_equals(arg, "--project")?)
            }
            _ if arg.starts_with("--tool=") => {
                parsed.tool = Some(value_after_equals(arg, "--tool")?)
            }
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(norm_time(value_after_equals(arg, "--from")?)?)
            }
            _ if arg.starts_with("--to=") => {
                parsed.to = Some(norm_time(value_after_equals(arg, "--to")?)?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown sessions option: {arg}"),
            _ => bail!("unexpected sessions argument: {arg}"),
        }
    }
    Ok(CliCommand::Sessions(parsed))
}

pub(crate) fn parse_incident(args: &[String]) -> Result<CliCommand> {
    let mut parsed = IncidentArgs {
        minutes: Some(5),
        limit: Some(500),
        ..Default::default()
    };
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--around" => parsed.around = norm_time(flags.value("--around")?)?,
            "--minutes" => {
                parsed.minutes = Some(parse_u32_flag("--minutes", flags.value("--minutes")?)?)
            }
            "--service" => parsed.service = Some(flags.value("--service")?),
            "--hostname" | "--host" => parsed.hostname = Some(flags.value(&arg)?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--around=") => {
                parsed.around = norm_time(value_after_equals(arg, "--around")?)?
            }
            _ if arg.starts_with("--minutes=") => {
                parsed.minutes = Some(parse_u32_flag(
                    "--minutes",
                    value_after_equals(arg, "--minutes")?,
                )?)
            }
            _ if arg.starts_with("--service=") => {
                parsed.service = Some(value_after_equals(arg, "--service")?)
            }
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--host=") => {
                parsed.hostname = Some(value_after_equals(arg, "--host")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown incident option: {arg}"),
            _ => bail!("unexpected incident argument: {arg}"),
        }
    }
    if parsed.around.is_empty() {
        bail!("incident requires --around <RFC3339>");
    }
    Ok(CliCommand::Incident(parsed))
}

pub(crate) fn parse_correlate(args: &[String]) -> Result<CliCommand> {
    let mut parsed = CorrelateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--reference-time" => {
                parsed.reference_time = norm_time(flags.value("--reference-time")?)?
            }
            "--window-minutes" => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    flags.value("--window-minutes")?,
                )?)
            }
            "--severity-min" => parsed.severity_min = Some(flags.value("--severity-min")?),
            "--hostname" => parsed.hostname = Some(flags.value("--hostname")?),
            "--source-ip" => parsed.source_ip = Some(flags.value("--source-ip")?),
            "--query" => parsed.query = Some(flags.value("--query")?),
            "--limit" => parsed.limit = Some(parse_u32_flag("--limit", flags.value("--limit")?)?),
            _ if arg.starts_with("--reference-time=") => {
                parsed.reference_time = norm_time(value_after_equals(arg, "--reference-time")?)?
            }
            _ if arg.starts_with("--window-minutes=") => {
                parsed.window_minutes = Some(parse_u32_flag(
                    "--window-minutes",
                    value_after_equals(arg, "--window-minutes")?,
                )?)
            }
            _ if arg.starts_with("--severity-min=") => {
                parsed.severity_min = Some(value_after_equals(arg, "--severity-min")?)
            }
            _ if arg.starts_with("--hostname=") => {
                parsed.hostname = Some(value_after_equals(arg, "--hostname")?)
            }
            _ if arg.starts_with("--source-ip=") => {
                parsed.source_ip = Some(value_after_equals(arg, "--source-ip")?)
            }
            _ if arg.starts_with("--query=") => {
                parsed.query = Some(value_after_equals(arg, "--query")?)
            }
            _ if arg.starts_with("--limit=") => {
                parsed.limit = Some(parse_u32_flag(
                    "--limit",
                    value_after_equals(arg, "--limit")?,
                )?)
            }
            _ if arg.starts_with('-') => bail!("unknown correlate option: {arg}"),
            _ if parsed.reference_time.is_empty() => parsed.reference_time = norm_time(arg)?,
            _ => bail!("unexpected correlate argument: {arg}"),
        }
    }
    if parsed.reference_time.is_empty() {
        bail!("correlate requires --reference-time <RFC3339>");
    }
    Ok(CliCommand::Correlate(parsed))
}

pub(crate) fn parse_source_ips(args: &[String]) -> Result<CliCommand> {
    let mut parsed = SourceIpsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--limit")? {
            parsed.limit = Some(parse_u32_flag("--limit", v)?);
        } else if let Some(v) = flags.match_value(&arg, "--offset")? {
            parsed.offset = Some(parse_u32_flag("--offset", v)?);
        } else {
            bail!("unknown source-ips option: {arg}");
        }
    }
    Ok(CliCommand::SourceIps(parsed))
}

pub(crate) fn parse_timeline(args: &[String]) -> Result<CliCommand> {
    let mut parsed = TimelineArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--bucket")? {
            parsed.bucket = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--group-by")? {
            parsed.group_by = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--from")? {
            parsed.from = Some(norm_time(v)?);
        } else if let Some(v) = flags.match_value(&arg, "--to")? {
            parsed.to = Some(norm_time(v)?);
        } else if let Some(v) = flags.match_value(&arg, "--hostname")? {
            parsed.hostname = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--app-name")? {
            parsed.app_name = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--severity-min")? {
            parsed.severity_min = Some(v);
        } else {
            bail!("unknown timeline option: {arg}");
        }
    }
    Ok(CliCommand::Timeline(parsed))
}

pub(crate) fn parse_patterns(args: &[String]) -> Result<CliCommand> {
    let mut parsed = PatternsArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        if arg == "--json" {
            parsed.json = true;
        } else if let Some(v) = flags.match_value(&arg, "--from")? {
            parsed.from = Some(norm_time(v)?);
        } else if let Some(v) = flags.match_value(&arg, "--to")? {
            parsed.to = Some(norm_time(v)?);
        } else if let Some(v) = flags.match_value(&arg, "--hostname")? {
            parsed.hostname = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--app-name")? {
            parsed.app_name = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--severity-min")? {
            parsed.severity_min = Some(v);
        } else if let Some(v) = flags.match_value(&arg, "--scan-limit")? {
            parsed.scan_limit = Some(parse_u32_flag("--scan-limit", v)?);
        } else if let Some(v) = flags.match_value(&arg, "--top-n")? {
            parsed.top_n = Some(parse_u32_flag("--top-n", v)?);
        } else if let Some(v) = flags.match_value(&arg, "--limit")? {
            parsed.top_n = Some(parse_u32_flag("--limit", v)?);
        } else {
            bail!("unknown patterns option: {arg}");
        }
    }
    Ok(CliCommand::Patterns(parsed))
}

pub(crate) fn parse_ingest_rate(args: &[String]) -> Result<CliCommand> {
    let mut parsed = IngestRateArgs::default();
    let mut flags = FlagCursor::new(args);
    while let Some(arg) = flags.next() {
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--by-host" => parsed.by_host = true,
            _ => bail!("unknown ingest-rate option: {arg}"),
        }
    }
    Ok(CliCommand::IngestRate(parsed))
}

#[cfg(test)]
#[path = "parse_logs_tests.rs"]
mod tests;
