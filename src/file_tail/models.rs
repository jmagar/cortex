use serde::{Deserialize, Serialize};

const SYSLOG_FACILITIES: &[&str] = &[
    "kern",
    "user",
    "mail",
    "daemon",
    "auth",
    "syslog",
    "lpr",
    "news",
    "uucp",
    "cron",
    "authpriv",
    "ftp",
    "ntp",
    "security",
    "console",
    "solaris-cron",
    "local0",
    "local1",
    "local2",
    "local3",
    "local4",
    "local5",
    "local6",
    "local7",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileTailSource {
    pub id: String,
    pub path: String,
    pub tag: String,
    pub hostname: Option<String>,
    pub facility: Option<String>,
    pub severity: String,
    pub start_at_end: bool,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_dev: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_ino: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_offset: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileTailOp {
    List,
    Add,
    Remove,
    Enable,
    Disable,
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileTailRequest {
    pub op: FileTailOp,
    pub id: Option<String>,
    pub path: Option<String>,
    pub tag: Option<String>,
    pub hostname: Option<String>,
    pub facility: Option<String>,
    pub severity: Option<String>,
    pub start_at_end: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileTailAddRequest {
    pub id: String,
    pub path: String,
    pub tag: String,
    pub hostname: Option<String>,
    pub facility: Option<String>,
    pub severity: Option<String>,
    pub start_at_end: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTailStatus {
    pub id: String,
    pub running: bool,
    pub last_line_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTailResponse {
    pub sources: Vec<FileTailSource>,
    pub statuses: Vec<FileTailStatus>,
}

impl FileTailSource {
    pub(crate) fn from_add(req: FileTailAddRequest, now: &str) -> Self {
        Self {
            id: req.id,
            path: req.path,
            tag: req.tag,
            hostname: req.hostname,
            facility: Some(req.facility.unwrap_or_else(|| "local7".to_string())),
            severity: normalize_severity(req.severity.as_deref())
                .unwrap_or_else(|| "info".to_string()),
            start_at_end: req.start_at_end.unwrap_or(true),
            enabled: true,
            checkpoint_dev: None,
            checkpoint_ino: None,
            checkpoint_offset: None,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        }
    }

    pub(crate) fn same_definition(&self, other: &Self) -> bool {
        self.id == other.id
            && self.path == other.path
            && self.tag == other.tag
            && self.hostname == other.hostname
            && self.facility == other.facility
            && self.severity == other.severity
            && self.start_at_end == other.start_at_end
            && self.enabled == other.enabled
    }
}

impl FileTailRequest {
    pub(crate) fn validate(&self) -> Result<(), String> {
        match self.op {
            FileTailOp::List | FileTailOp::Status => Ok(()),
            FileTailOp::Add => {
                let Some(id) = self.id.as_deref() else {
                    return Err("file_tails op=add requires id, path, and tag".into());
                };
                validate_id(id)?;
                if self.path.as_deref().is_none_or(str::is_empty)
                    || self.tag.as_deref().is_none_or(str::is_empty)
                {
                    return Err("file_tails op=add requires id, path, and tag".into());
                }
                if let Some(severity) = self.severity.as_deref() {
                    normalize_severity(Some(severity)).ok_or_else(|| {
                        "file_tails severity must be one of emerg, alert, crit, err, warning, notice, info, debug".to_string()
                    })?;
                }
                if let Some(facility) = self.facility.as_deref() {
                    validate_facility(facility)?;
                }
                Ok(())
            }
            FileTailOp::Remove | FileTailOp::Enable | FileTailOp::Disable => {
                let Some(id) = self.id.as_deref() else {
                    return Err(format!("file_tails op={:?} requires id", self.op).to_lowercase());
                };
                validate_id(id)
            }
        }
    }

    pub(crate) fn into_add(self) -> Result<FileTailAddRequest, String> {
        self.validate()?;
        Ok(FileTailAddRequest {
            id: self.id.expect("validated id"),
            path: self.path.expect("validated path"),
            tag: self.tag.expect("validated tag"),
            hostname: self.hostname,
            facility: self.facility,
            severity: self.severity,
            start_at_end: self.start_at_end,
        })
    }
}

fn normalize_severity(severity: Option<&str>) -> Option<String> {
    let severity = severity?;
    match severity.to_ascii_lowercase().as_str() {
        "emerg" | "emergency" => Some("emerg".to_string()),
        "alert" => Some("alert".to_string()),
        "crit" | "critical" => Some("crit".to_string()),
        "err" | "error" | "fatal" | "panic" => Some("err".to_string()),
        "warning" | "warn" => Some("warning".to_string()),
        "notice" => Some("notice".to_string()),
        "info" | "informational" => Some("info".to_string()),
        "debug" => Some("debug".to_string()),
        _ => None,
    }
}

fn validate_facility(facility: &str) -> Result<(), String> {
    if SYSLOG_FACILITIES.contains(&facility) {
        return Ok(());
    }
    Err("file_tails facility must be a canonical syslog facility".into())
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(
            "file_tails id must contain only ASCII letters, digits, dot, underscore, or dash"
                .into(),
        );
    }
    Ok(())
}
