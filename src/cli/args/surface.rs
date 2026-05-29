#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SilentHostsArgs {
    pub silent_minutes: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ClockSkewArgs {
    pub since: Option<String>,
    pub limit: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AnomaliesArgs {
    pub recent_minutes: Option<u32>,
    pub baseline_minutes: Option<u32>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CompareArgs {
    pub a_from: Option<String>,
    pub a_to: Option<String>,
    pub b_from: Option<String>,
    pub b_to: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AppsArgs {
    pub hostname: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub json: bool,
}
