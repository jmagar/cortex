pub(crate) mod models;

pub(crate) use models::{
    FileTailAddRequest, FileTailOp, FileTailRequest, FileTailResponse, FileTailSource,
    FileTailStatus,
};

#[cfg(test)]
#[path = "file_tail/models_tests.rs"]
mod models_tests;
