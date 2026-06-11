use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileTailSource {
    pub id: String,
    pub path: String,
    pub tag: String,
    pub hostname: Option<String>,
    pub facility: Option<String>,
    pub severity: String,
    pub start_at_end: bool,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FileTailOp {
    List,
    Add,
    Remove,
    Enable,
    Disable,
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileTailRequest {
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
pub(crate) struct FileTailAddRequest {
    pub id: String,
    pub path: String,
    pub tag: String,
    pub hostname: Option<String>,
    pub facility: Option<String>,
    pub severity: Option<String>,
    pub start_at_end: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct FileTailStatus {
    pub id: String,
    pub running: bool,
    pub last_line_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct FileTailResponse {
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
            severity: req.severity.unwrap_or_else(|| "info".to_string()),
            start_at_end: req.start_at_end.unwrap_or(true),
            enabled: true,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        }
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
