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
    pub host: Option<String>,
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
    pub host: Option<String>,
    pub facility: Option<String>,
    pub severity: Option<String>,
    pub start_at_end: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTailStatus {
    pub id: String,
    pub running: bool,
    pub last_line_at: Option<String>,
    pub last_read_at: Option<String>,
    pub last_checkpoint_at: Option<String>,
    pub blocked_on_writer_since: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTailResponse {
    pub sources: Vec<FileTailSource>,
    pub statuses: Vec<FileTailStatus>,
}

impl FileTailSource {
    pub(crate) fn from_add(req: FileTailAddRequest, now: &str) -> Result<Self, String> {
        validate_id(&req.id)?;
        if req.path.is_empty() || req.tag.is_empty() || req.host.is_none() {
            return Err("file_tails op=add requires id, path, tag, and host".into());
        }
        if let Some(facility) = req.facility.as_deref() {
            validate_facility(facility)?;
        }
        let severity = req
            .severity
            .as_deref()
            .map(|severity| {
                normalize_severity(Some(severity)).ok_or_else(|| {
                    "file_tails severity must be one of emerg, alert, crit, err, warning, notice, info, debug".to_string()
                })
            })
            .transpose()?
            .unwrap_or_else(|| "info".to_string());

        let hostname = req
            .host
            .as_deref()
            .ok_or_else(|| "file_tails op=add requires id, path, tag, and host".to_string())
            .and_then(normalize_hostname)?;

        Ok(Self {
            id: req.id,
            path: req.path,
            tag: req.tag,
            hostname: Some(hostname),
            facility: Some(req.facility.unwrap_or_else(|| "local7".to_string())),
            severity,
            start_at_end: req.start_at_end.unwrap_or(true),
            enabled: true,
            checkpoint_dev: None,
            checkpoint_ino: None,
            checkpoint_offset: None,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        })
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
    pub fn list() -> Self {
        Self {
            op: FileTailOp::List,
            id: None,
            path: None,
            tag: None,
            host: None,
            facility: None,
            severity: None,
            start_at_end: None,
        }
    }

    pub fn status() -> Self {
        Self {
            op: FileTailOp::Status,
            ..Self::list()
        }
    }

    pub fn id_op(op: FileTailOp, id: String) -> Self {
        Self {
            op,
            id: Some(id),
            path: None,
            tag: None,
            host: None,
            facility: None,
            severity: None,
            start_at_end: None,
        }
    }

    pub fn add(add: FileTailAddRequest) -> Self {
        Self {
            op: FileTailOp::Add,
            id: Some(add.id),
            path: Some(add.path),
            tag: Some(add.tag),
            host: add.host,
            facility: add.facility,
            severity: add.severity,
            start_at_end: add.start_at_end,
        }
    }

    pub(crate) fn required_id(&self) -> Result<&str, String> {
        let id = self
            .id
            .as_deref()
            .ok_or_else(|| format!("file_tails op={:?} requires id", self.op).to_lowercase())?;
        validate_id(id)?;
        Ok(id)
    }

    pub(crate) fn validate_shape(&self) -> Result<(), String> {
        match self.op {
            FileTailOp::List | FileTailOp::Status => {
                if self.id.is_some()
                    || self.path.is_some()
                    || self.tag.is_some()
                    || self.host.is_some()
                    || self.facility.is_some()
                    || self.severity.is_some()
                    || self.start_at_end.is_some()
                {
                    return Err(format!(
                        "file_tails op={:?} does not accept source fields",
                        self.op
                    )
                    .to_lowercase());
                }
                Ok(())
            }
            FileTailOp::Add => Ok(()),
            FileTailOp::Remove | FileTailOp::Enable | FileTailOp::Disable => {
                if self.path.is_some()
                    || self.tag.is_some()
                    || self.host.is_some()
                    || self.facility.is_some()
                    || self.severity.is_some()
                    || self.start_at_end.is_some()
                {
                    return Err(
                        format!("file_tails op={:?} accepts only id", self.op).to_lowercase()
                    );
                }
                Ok(())
            }
        }
    }

    pub(crate) fn into_add(self) -> Result<FileTailAddRequest, String> {
        let id = self
            .id
            .ok_or_else(|| "file_tails op=add requires id, path, tag, and host".to_string())?;
        validate_id(&id)?;
        let path = self
            .path
            .ok_or_else(|| "file_tails op=add requires id, path, tag, and host".to_string())?;
        let tag = self
            .tag
            .ok_or_else(|| "file_tails op=add requires id, path, tag, and host".to_string())?;
        let hostname = self
            .host
            .ok_or_else(|| "file_tails op=add requires id, path, tag, and host".to_string())?;
        if path.is_empty() || tag.is_empty() || hostname.trim().is_empty() {
            return Err("file_tails op=add requires id, path, tag, and host".into());
        }
        Ok(FileTailAddRequest {
            id,
            path,
            tag,
            host: Some(hostname),
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

fn normalize_hostname(hostname: &str) -> Result<String, String> {
    let hostname = hostname.trim().to_ascii_lowercase();
    if hostname.is_empty()
        || hostname.len() > 255
        || !hostname
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        || hostname.starts_with(['.', '-', '_'])
        || hostname.ends_with(['.', '-', '_'])
    {
        return Err(
            "file_tails hostname must be URI-safe ASCII letters, digits, dot, underscore, or dash"
                .into(),
        );
    }
    Ok(hostname)
}
