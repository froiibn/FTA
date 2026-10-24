//! Explicit directory overrides isolate test instances without changing process-wide environment.
use crate::{Error, Result};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

pub const DEFAULT_PORT: u16 = 47321;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_CONTROL_BYTES: usize = 8 * MAX_MESSAGE_BYTES;
pub const MAX_ITEMS: usize = 100;
pub const IDLE_SECONDS: u64 = 60;
pub const CONNECT_SECONDS: u64 = 5;
pub const PAIR_SECONDS: u64 = 120;
pub const MAX_STREAMS: usize = 2;

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub default_receive_dir: PathBuf,
}
impl AppPaths {
    /// OS paths avoid assumptions about usernames or translated home directories.
    pub fn discover() -> Result<Self> {
        let base = fta_platform::paths()?;
        Ok(Self {
            config_dir: env_path("FTA_CONFIG_DIR", base.config_dir),
            state_dir: env_path("FTA_STATE_DIR", base.state_dir),
            runtime_dir: env_path("FTA_RUNTIME_DIR", base.runtime_dir),
            default_receive_dir: env_path("FTA_RECEIVE_DIR", base.receive_dir),
        })
    }
    pub fn prepare(&self) -> Result<()> {
        for path in [&self.config_dir, &self.state_dir, &self.runtime_dir] {
            private_dir(path)?;
        }
        Ok(())
    }
    pub fn socket_path(&self) -> PathBuf {
        self.runtime_dir.join("engine.sock")
    }
    #[cfg(windows)]
    pub fn pipe_name(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        fta_platform::windows::user_pipe_name(&hex::encode(Sha256::digest(
            self.runtime_dir.to_string_lossy().as_bytes(),
        )))
        .map_err(Into::into)
    }
}
fn env_path(key: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(key).map(PathBuf::from).unwrap_or(fallback)
}

/// Unconfigured installs listen only on loopback until the user explicitly selects a home LAN.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub device_id: String,
    pub display_name: String,
    pub receive_dir: PathBuf,
    pub listen_addr: SocketAddr,
    pub allowed_networks: Vec<IpNet>,
    pub discovery_enabled: bool,
}
impl Config {
    pub fn load_or_create(paths: &AppPaths) -> Result<Self> {
        paths.prepare()?;
        let file = paths.config_dir.join("config.json");
        if file.exists() {
            return Ok(serde_json::from_slice(&fs::read(file)?)?);
        }
        let config =
            Self {
                device_id: uuid::Uuid::new_v4().to_string(),
                display_name: std::env::var("COMPUTERNAME")
                    .or_else(|_| std::env::var("HOSTNAME"))
                    .unwrap_or_else(|_| "My computer".into()),
                receive_dir: paths.default_receive_dir.clone(),
                listen_addr: SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)),
                allowed_networks: vec!["127.0.0.0/8".parse().map_err(
                    |e: ipnet::AddrParseError| Error::new("CONFIG_ERROR", e.to_string()),
                )?],
                discovery_enabled: false,
            };
        config.save(paths)?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        if self.display_name.trim().is_empty() || self.display_name.len() > 128 {
            return Err(Error::new(
                "INVALID_REQUEST",
                "device name must contain 1..128 bytes",
            ));
        }
        if !self.receive_dir.is_absolute() {
            return Err(Error::new(
                "INVALID_REQUEST",
                "receive directory must be absolute",
            ));
        }
        if self.allowed_networks.is_empty()
            || self.allowed_networks.iter().any(|n| n.prefix_len() == 0)
        {
            return Err(Error::new(
                "INVALID_REQUEST",
                "explicit local network CIDRs are required",
            ));
        }
        if !self
            .allowed_networks
            .iter()
            .any(|n| n.contains(&self.listen_addr.ip()))
        {
            return Err(Error::new(
                "INVALID_REQUEST",
                "listener must bind an address inside an allowed network",
            ));
        }
        Ok(())
    }
    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        self.validate()?;
        paths.prepare()?;
        atomic_write(
            &paths.config_dir.join("config.json"),
            &serde_json::to_vec_pretty(self)?,
        )
    }
    pub fn allows(&self, ip: std::net::IpAddr) -> bool {
        self.allowed_networks.iter().any(|n| n.contains(&ip))
    }
}
/// Restrict identity and IPC directories before placing any credentials or sockets in them.
pub fn private_dir(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::new(
            "INVALID_PATH",
            "application directories must be absolute",
        ));
    }
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(Error::new(
            "INVALID_PATH",
            "private directory must not be a symlink",
        ));
    }
    fs::create_dir_all(path)?;
    #[cfg(windows)]
    fta_platform::windows::secure_directory(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // Replacing settings must preserve the pathname and never require a delete-first gap.
    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"second");
    }
    #[test]
    fn application_directories_reject_relative_and_symlink_paths() {
        assert!(private_dir(Path::new("relative")).is_err());
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(dir.path(), &link).unwrap();
            assert!(private_dir(&link).is_err());
        }
    }
}
