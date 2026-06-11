pub(crate) mod models;
pub(crate) mod registry;
pub(crate) mod supervisor;

pub(crate) use models::{
    FileTailAddRequest, FileTailOp, FileTailRequest, FileTailResponse, FileTailSource,
    FileTailStatus,
};
pub(crate) use registry::FileTailRegistry;
pub(crate) use supervisor::FileTailSupervisor;

#[cfg(test)]
#[path = "file_tail/models_tests.rs"]
mod models_tests;

#[cfg(test)]
#[path = "file_tail/registry_tests.rs"]
mod registry_tests;

#[cfg(test)]
#[path = "file_tail/supervisor_tests.rs"]
mod supervisor_tests;
