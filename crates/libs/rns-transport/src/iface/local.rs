use alloc::string::String;
#[cfg(any(target_os = "linux", target_os = "android"))]
use std::ffi::OsString;
#[cfg(any(target_os = "linux", target_os = "android"))]
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{UnixListener, UnixStream};

use super::tcp_client::{run_hdlc_stream_with_runtime_and_ifac, HdlcStreamRuntime, TcpClient};
use super::{IfacState, Interface, InterfaceContext, InterfaceManager};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalUnixEndpoint {
    Filesystem(PathBuf),
    Abstract(String),
}

impl LocalUnixEndpoint {
    pub fn filesystem<P: Into<PathBuf>>(path: P) -> Self {
        Self::Filesystem(path.into())
    }

    pub fn abstract_name<T: Into<String>>(name: T) -> Self {
        let mut name = name.into();
        if let Some(stripped) = name.strip_prefix('@') {
            name = stripped.to_string();
        }
        if let Some(stripped) = name.strip_prefix('\0') {
            name = stripped.to_string();
        }
        Self::Abstract(name)
    }

    #[must_use]
    pub fn from_config_value(value: &str) -> Self {
        if let Some(stripped) = value.strip_prefix('@') {
            Self::abstract_name(stripped)
        } else if let Some(stripped) = value.strip_prefix('\0') {
            Self::abstract_name(stripped)
        } else {
            Self::filesystem(value)
        }
    }

    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Filesystem(path) => path.display().to_string(),
            Self::Abstract(name) => format!("@{name}"),
        }
    }

    #[must_use]
    pub fn is_abstract(&self) -> bool {
        matches!(self, Self::Abstract(_))
    }

    fn filesystem_path(&self) -> Option<&Path> {
        match self {
            Self::Filesystem(path) => Some(path.as_path()),
            Self::Abstract(_) => None,
        }
    }

    fn tokio_path(&self) -> PathBuf {
        match self {
            Self::Filesystem(path) => path.clone(),
            Self::Abstract(name) => abstract_socket_path(name),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn abstract_socket_path(name: &str) -> PathBuf {
    let mut bytes = Vec::with_capacity(name.len() + 1);
    bytes.push(0);
    bytes.extend_from_slice(name.as_bytes());
    PathBuf::from(OsString::from_vec(bytes))
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn abstract_socket_path(name: &str) -> PathBuf {
    PathBuf::from(format!("@{name}"))
}

include!("local_parts/server.rs");

include!("local_parts/client.rs");

fn prepare_socket_path(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    remove_socket_file(path)
}

fn remove_socket_file(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_socket() {
                std::fs::remove_file(path)
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "path exists and is not a unix socket",
                ))
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
#[path = "local/tests/runtime.rs"]
mod tests;
