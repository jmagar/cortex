//! V1 parser implementations — one module per recognised log source.

pub mod authelia;
pub mod docker_event;
pub mod kernel;

pub use authelia::AutheliaParser;
pub use docker_event::DockerEventParser;
pub use kernel::KernelParser;
