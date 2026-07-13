use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::setup::cortex_home_dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateScope {
    All,
    Server,
    Clients,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateOptions {
    pub dry_run: bool,
    pub profile_path: Option<PathBuf>,
    pub binary: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerUpdateProfile {
    pub host: String,
    pub home: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClientsUpdateProfile {
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub docker: Option<bool>,
    #[serde(default)]
    pub journald: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UpdateProfile {
    #[serde(default)]
    pub server: Option<ServerUpdateProfile>,
    #[serde(default)]
    pub clients: ClientsUpdateProfile,
}

pub fn default_profile_path() -> io::Result<PathBuf> {
    Ok(cortex_home_dir()?.join("deployments.toml"))
}

pub fn load_profile(path: &Path) -> io::Result<UpdateProfile> {
    match std::fs::read_to_string(path) {
        Ok(raw) => toml::from_str(&raw).map_err(|error| {
            io::Error::new(
                ErrorKind::InvalidData,
                format!("parse update profile {}: {error}", path.display()),
            )
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(UpdateProfile::default()),
        Err(error) => Err(error),
    }
}

pub fn write_profile(path: &Path, profile: &UpdateProfile) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let rendered = toml::to_string_pretty(profile).map_err(|error| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("render update profile {}: {error}", path.display()),
        )
    })?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, rendered)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

pub fn configure_server_profile(
    path: Option<&Path>,
    host: &str,
    home: &str,
) -> io::Result<UpdateProfile> {
    let path = resolve_profile_path(path)?;
    let mut profile = load_profile(&path)?;
    profile.server = Some(ServerUpdateProfile {
        host: validate_host(host)?,
        home: validate_remote_home(home)?,
    });
    write_profile(&path, &profile)?;
    Ok(profile)
}

pub fn configure_clients_profile(
    path: Option<&Path>,
    hosts: Vec<String>,
    target: Option<String>,
    docker: Option<bool>,
    journald: Option<bool>,
) -> io::Result<UpdateProfile> {
    let path = resolve_profile_path(path)?;
    let mut validated = Vec::new();
    for host in hosts {
        validated.push(validate_host(&host)?);
    }
    if validated.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "at least one client host is required",
        ));
    }
    let mut profile = load_profile(&path)?;
    profile.clients = ClientsUpdateProfile {
        hosts: validated,
        target,
        docker,
        journald,
    };
    write_profile(&path, &profile)?;
    Ok(profile)
}

fn resolve_profile_path(path: Option<&Path>) -> io::Result<PathBuf> {
    match path {
        Some(path) => Ok(path.to_path_buf()),
        None => default_profile_path(),
    }
}

fn validate_host(host: &str) -> io::Result<String> {
    let trimmed = host.trim();
    if trimmed.is_empty() || !crate::inventory::ssh::is_safe_ssh_host(trimmed) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unsafe ssh host: {host}"),
        ));
    }
    Ok(trimmed.to_string())
}

fn validate_remote_home(home: &str) -> io::Result<String> {
    let trimmed = home.trim();
    let path = Path::new(trimmed);
    if trimmed.is_empty() || !path.is_absolute() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "server home must be a non-empty absolute path",
        ));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "server home must not contain '..'",
        ));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
