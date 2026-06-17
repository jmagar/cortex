//! Canonical CLI flag metadata, shared by the parser, completion, and help.

/// One CLI flag for an action. `value_kind` drives dynamic completion.
#[derive(Debug, Clone, Copy)]
pub struct FlagSpec {
    /// Canonical long flag, including leading dashes, e.g. "--host".
    pub flag: &'static str,
    /// Optional short alias, e.g. "-n". Empty string = none.
    pub short: &'static str,
    /// One-line help.
    pub help: &'static str,
    /// Completion source for the flag's value.
    pub value_kind: ValueKind,
}

/// What completes after a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// No value (boolean flag).
    None,
    /// Free text (no candidates).
    Text,
    /// Live hostnames from the DB.
    Host,
    /// Live app names from the DB.
    App,
    /// Live source identifiers from the DB.
    Source,
    /// Fixed enum candidates.
    Enum(&'static [&'static str]),
    /// A time value (offers relative hints).
    Time,
}

/// Zero-flag defaults applied to an action when the user omits them.
///
/// Kept deliberately small: the only knobs worth defaulting at the CLI layer
/// are the result `limit` and the start-of-window `since`. `None` in either
/// field means "leave it to the server / unbounded" — i.e. no default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Defaults {
    /// Default `--limit` when the user passes none (`None` = server default).
    pub limit: Option<u32>,
    /// Default `--since` window when unset, expressed as a relative literal the
    /// time parser understands (e.g. `"1h"`). `None` = unbounded.
    pub since: Option<&'static str>,
}

impl Defaults {
    /// Const constructor for the empty (no-defaults) case, usable inside the
    /// `const ACTION_SPECS` table where `Default::default()` cannot be called.
    pub const fn new() -> Self {
        Self {
            limit: None,
            since: None,
        }
    }
}

pub(super) const SEVERITIES: &[&str] = &[
    "emerg", "alert", "crit", "err", "warning", "notice", "info", "debug",
];

/// Flags shared by the log-query actions (search/filter/tail/errors/...).
pub(super) const COMMON_LOG_FLAGS: &[FlagSpec] = &[
    FlagSpec {
        flag: "--host",
        short: "",
        help: "Filter by hostname",
        value_kind: ValueKind::Host,
    },
    FlagSpec {
        flag: "--app",
        short: "",
        help: "Filter by app/program name",
        value_kind: ValueKind::App,
    },
    FlagSpec {
        flag: "--source",
        short: "",
        help: "Filter by source id (IP:port or docker://...)",
        value_kind: ValueKind::Source,
    },
    FlagSpec {
        flag: "--severity",
        short: "-s",
        help: "Minimum severity",
        value_kind: ValueKind::Enum(SEVERITIES),
    },
    FlagSpec {
        flag: "--since",
        short: "",
        help: "Start of window (1h, 2d, yesterday, RFC3339)",
        value_kind: ValueKind::Time,
    },
    FlagSpec {
        flag: "--until",
        short: "",
        help: "End of window",
        value_kind: ValueKind::Time,
    },
    FlagSpec {
        flag: "--limit",
        short: "-n",
        help: "Max results",
        value_kind: ValueKind::Text,
    },
    FlagSpec {
        flag: "--json",
        short: "",
        help: "JSON output",
        value_kind: ValueKind::None,
    },
];
