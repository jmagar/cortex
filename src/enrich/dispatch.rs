//! Dispatcher — picks a parser per (source_kind, app_name, container_name)
//! and merges its output onto the entry.
//!
//! Spec: docs/superpowers/specs/2026-05-16-enrichment-framework-design.md §4

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Mutex;

use lru::LruCache;
use serde_json::Value;

use crate::db::LogBatchEntry;
use crate::enrich::output::{merge_output, record_error};
use crate::enrich::parsers::{
    AdguardParser, AutheliaParser, DockerEventParser, Fail2banParser, KernelParser, SwagParser,
};
use crate::enrich::{Parser, ParserInput, SourceKind};

const LRU_CAP: usize = 256;

/// Maps operator-renamed container names to canonical parser keys.
fn container_to_canonical(container: &str) -> &'static str {
    match container {
        "authelia" | "authelia-main" | "authelia-prod" | "authelia-master" => "authelia",
        "swag" | "swag-main" | "nginx" | "nginx-proxy" => "swag",
        "adguardhome" | "adguard" | "adguardhome-main" => "adguard",
        "fail2ban" | "fail2ban-main" => "fail2ban",
        _ => "",
    }
}

// Static singleton parser instances.
static KERNEL: KernelParser = KernelParser;
static DOCKER_EVENT: DockerEventParser = DockerEventParser;
static AUTHELIA: AutheliaParser = AutheliaParser;
static SWAG: SwagParser = SwagParser;
static ADGUARD: AdguardParser = AdguardParser;
static FAIL2BAN: Fail2banParser = Fail2banParser;

pub struct EnrichmentPipeline {
    by_name: HashMap<&'static str, &'static dyn Parser>,
    docker_event: &'static DockerEventParser,
    unknown_apps: Mutex<LruCache<String, ()>>,
}

impl EnrichmentPipeline {
    pub fn new() -> Self {
        let mut by_name: HashMap<&'static str, &'static dyn Parser> = HashMap::new();
        by_name.insert("kernel", &KERNEL);
        by_name.insert("authelia", &AUTHELIA);
        by_name.insert("swag", &SWAG);
        by_name.insert("adguard", &ADGUARD);
        by_name.insert("adguard-query", &ADGUARD); // API poller app_name
        by_name.insert("fail2ban", &FAIL2BAN);

        Self {
            by_name,
            docker_event: &DOCKER_EVENT,
            unknown_apps: Mutex::new(LruCache::new(
                NonZeroUsize::new(LRU_CAP).expect("LRU_CAP > 0"),
            )),
        }
    }

    pub fn dispatch(&self, entry: &mut LogBatchEntry) {
        let source_kind = read_source_kind(entry);

        // docker-event short-circuit — routed by source_kind, not app_name.
        if source_kind.as_deref() == Some("docker-event") {
            self.apply(entry, self.docker_event);
            return;
        }

        // Container-name lookup (higher priority than app_name for Docker sources).
        let container = read_container_name(entry);
        if let Some(c) = container.as_deref() {
            let canon = container_to_canonical(c);
            if !canon.is_empty() {
                if let Some(&parser) = self.by_name.get(canon) {
                    self.apply(entry, parser);
                    return;
                }
            }
        }

        // app_name fallback.
        let app_lower = entry.app_name.as_deref().map(|s| s.to_ascii_lowercase());
        if let Some(app) = app_lower.as_deref() {
            if let Some(&parser) = self.by_name.get(app) {
                self.apply(entry, parser);
                return;
            }
            // Debug-log unknown app_names once per unique value.
            if let Ok(mut lru) = self.unknown_apps.lock() {
                if lru.put(app.to_string(), ()).is_none() {
                    tracing::debug!(app_name = app, "enrich: no parser registered for app_name");
                }
            }
        }
    }

    fn apply(&self, entry: &mut LogBatchEntry, parser: &'static dyn Parser) {
        let source_kind = parse_source_kind(entry);
        let container = read_container_name(entry);
        let input = ParserInput {
            app_name: entry.app_name.as_deref(),
            container_name: container.as_deref(),
            message: &entry.message.clone(),
            raw: &entry.raw.clone(),
            source_kind,
            severity: &entry.severity.clone(),
        };
        match parser.parse(input) {
            Ok(out) => merge_output(entry, parser.namespace(), out),
            Err(e) => record_error(entry, parser.name(), &e.to_string()),
        }
    }
}

impl Default for EnrichmentPipeline {
    fn default() -> Self {
        Self::new()
    }
}

fn read_source_kind(entry: &LogBatchEntry) -> Option<String> {
    let raw = entry.metadata_json.as_deref()?;
    let v: Value = serde_json::from_str(raw).ok()?;
    v.get("source_kind")?.as_str().map(str::to_string)
}

fn parse_source_kind(entry: &LogBatchEntry) -> SourceKind {
    match read_source_kind(entry).as_deref() {
        Some("syslog-udp") => SourceKind::SyslogUdp,
        Some("syslog-tcp") => SourceKind::SyslogTcp,
        Some("docker-stream") => SourceKind::DockerStream,
        Some("docker-event") => SourceKind::DockerEvent,
        Some("otlp") => SourceKind::Otlp,
        Some("adguard-api") => SourceKind::AdguardApi,
        Some("unifi-api") => SourceKind::UnifiApi,
        Some("agent") => SourceKind::Agent,
        _ => SourceKind::SyslogTcp,
    }
}

fn read_container_name(entry: &LogBatchEntry) -> Option<String> {
    let raw = entry.metadata_json.as_deref()?;
    let v: Value = serde_json::from_str(raw).ok()?;
    v.get("docker")?
        .get("container_name")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod dispatch_tests;
