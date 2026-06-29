pub(crate) mod models;
pub(crate) mod path_policy;
pub(crate) mod platform;
pub(crate) mod registry;
pub(crate) mod supervisor;

pub use models::{
    FileTailAddRequest, FileTailOp, FileTailRequest, FileTailResponse, FileTailSource,
    FileTailStatus,
};
pub(crate) use registry::FileTailRegistry;
pub(crate) use supervisor::FileTailSupervisor;

#[cfg(test)]
#[path = "filetail/models_tests.rs"]
mod models_tests;

#[cfg(test)]
#[path = "filetail/path_policy_tests.rs"]
mod path_policy_tests;

#[cfg(test)]
#[path = "filetail/registry_tests.rs"]
mod registry_tests;

#[cfg(test)]
#[path = "filetail/supervisor_tests.rs"]
mod supervisor_tests;
