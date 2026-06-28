#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SigCommand {
    List(SigListArgs),
    Ack(SigAckArgs),
    Unack(SigUnackArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NotifyCommand {
    Recent(NotifyRecentArgs),
    Test(NotifyTestArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AlertsCommand {
    Signatures(SigCommand),
    Notifications(NotifyCommand),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SigListArgs {
    pub limit: Option<u32>,
    pub include_acknowledged: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SigAckArgs {
    pub signature_hash: String,
    pub notes: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SigUnackArgs {
    pub signature_hash: String,
    pub reason: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NotifyRecentArgs {
    pub limit: Option<i64>,
    pub rule_id: Option<String>,
    pub since: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NotifyTestArgs {
    pub body: Option<String>,
    pub json: bool,
}
