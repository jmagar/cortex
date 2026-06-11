pub(crate) mod models;
pub(crate) mod registry;

pub(crate) use models::{
    FileTailAddRequest, FileTailOp, FileTailRequest, FileTailResponse, FileTailSource,
    FileTailStatus,
};
pub(crate) use registry::FileTailRegistry;

#[cfg(test)]
#[path = "file_tail/models_tests.rs"]
mod models_tests;

#[cfg(test)]
#[path = "file_tail/registry_tests.rs"]
mod registry_tests;
