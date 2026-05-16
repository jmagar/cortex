//! V1 parser implementations — one module per recognised log source.

pub mod docker_event;
pub mod kernel;

pub use docker_event::DockerEventParser;
pub use kernel::KernelParser;
