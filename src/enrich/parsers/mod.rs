//! V1 parser implementations — one module per recognised log source.

pub mod authelia;
pub mod docker_event;
pub mod kernel;
pub mod adguard;
pub mod swag;

pub use adguard::AdguardParser;
pub use authelia::AutheliaParser;
pub use docker_event::DockerEventParser;
pub use kernel::KernelParser;
pub use swag::SwagParser;
