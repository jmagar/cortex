use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigCommand {
    Get(ConfigGetArgs),
    Set(ConfigSetArgs),
    Unset(ConfigUnsetArgs),
    List(ConfigListArgs),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ConfigTarget {
    #[default]
    Auto,
    Env,
    Toml,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ConfigGetArgs {
    pub key: String,
    pub target: ConfigTarget,
    pub toml_path: Option<PathBuf>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ConfigSetArgs {
    pub key: String,
    pub value: String,
    pub target: ConfigTarget,
    pub toml_path: Option<PathBuf>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ConfigUnsetArgs {
    pub key: String,
    pub target: ConfigTarget,
    pub toml_path: Option<PathBuf>,
    pub json: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ConfigListArgs {
    pub target: ConfigTarget,
    pub toml_path: Option<PathBuf>,
    pub json: bool,
}

#[cfg(test)]
#[path = "args_config_tests.rs"]
mod tests;
