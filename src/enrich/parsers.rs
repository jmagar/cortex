//! V1 parser implementations — one module per recognised log source.

pub mod adguard;
pub mod authelia;
pub mod docker_event;
pub mod fail2ban;
pub mod kernel;
pub mod swag;

pub use adguard::AdguardParser;
pub use authelia::AutheliaParser;
pub use docker_event::DockerEventParser;
pub use fail2ban::Fail2banParser;
pub use kernel::KernelParser;
pub use swag::SwagParser;
