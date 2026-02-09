use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const ACTIVE_PROFILE_FILE: &str = "active_profile";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSettings {
    pub name: String,
    pub managed: bool,
    pub rpc: String,
    pub reticulumd_path: Option<String>,
    pub db_path: Option<String>,
    pub identity_path: Option<String>,
    pub transport: Option<String>,
}

impl Default for ProfileSettings {
    fn default() -> Self {
        Self {
            name: "default".into(),
            managed: false,
            rpc: "127.0.0.1:4243".into(),
            reticulumd_path: None,
            db_path: None,
            identity_path: None,
            transport: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProfilePaths {
    pub root: PathBuf,
    pub profile_toml: PathBuf,
    pub reticulum_toml: PathBuf,
    pub daemon_pid: PathBuf,
    pub daemon_log: PathBuf,
    pub identity_file: PathBuf,
    pub daemon_db: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterfaceEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReticulumConfig {
    #[serde(default)]
    pub interfaces: Vec<InterfaceEntry>,
}

pub fn config_root() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("LXMF_CONFIG_ROOT") {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let base = dirs::config_dir().ok_or_else(|| anyhow!("failed to resolve config directory"))?;
    Ok(base.join("lxmf"))
}

pub fn profiles_root() -> Result<PathBuf> {
    Ok(config_root()?.join("profiles"))
}

pub fn active_profile_path() -> Result<PathBuf> {
    Ok(config_root()?.join(ACTIVE_PROFILE_FILE))
}

pub fn profile_paths(name: &str) -> Result<ProfilePaths> {
    let root = profiles_root()?.join(name);
    Ok(ProfilePaths {
        profile_toml: root.join("profile.toml"),
        reticulum_toml: root.join("reticulum.toml"),
        daemon_pid: root.join("daemon.pid"),
        daemon_log: root.join("daemon.log"),
        identity_file: root.join("identity"),
        daemon_db: root.join("reticulum.db"),
        root,
    })
}

pub fn profile_exists(name: &str) -> Result<bool> {
    Ok(profile_paths(name)?.profile_toml.exists())
}

pub fn init_profile(name: &str, managed: bool, rpc: Option<String>) -> Result<ProfileSettings> {
    let paths = profile_paths(name)?;
    fs::create_dir_all(&paths.root).with_context(|| {
        format!(
            "failed to create profile directory {}",
            paths.root.display()
        )
    })?;

    let mut settings = ProfileSettings {
        name: name.to_string(),
        managed,
        ..ProfileSettings::default()
    };
    if let Some(rpc) = rpc {
        settings.rpc = rpc;
    }

    save_profile_settings(&settings)?;
    if !paths.reticulum_toml.exists() {
        save_reticulum_config(name, &ReticulumConfig::default())?;
    }

    Ok(settings)
}

pub fn list_profiles() -> Result<Vec<String>> {
    let root = profiles_root()?;
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut profiles = Vec::new();
    for entry in fs::read_dir(&root)
        .with_context(|| format!("failed to list profiles in {}", root.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name();
            profiles.push(name.to_string_lossy().to_string());
        }
    }
    profiles.sort();
    Ok(profiles)
}

pub fn load_profile_settings(name: &str) -> Result<ProfileSettings> {
    let path = profile_paths(name)?.profile_toml;
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("failed to read profile settings {}", path.display()))?;
    let mut settings: ProfileSettings = toml::from_str(&contents)
        .with_context(|| format!("invalid profile settings in {}", path.display()))?;
    settings.name = name.to_string();
    Ok(settings)
}

pub fn save_profile_settings(settings: &ProfileSettings) -> Result<()> {
    let paths = profile_paths(&settings.name)?;
    fs::create_dir_all(&paths.root)
        .with_context(|| format!("failed to create {}", paths.root.display()))?;
    let encoded = toml::to_string_pretty(settings).context("failed to encode profile.toml")?;
    fs::write(&paths.profile_toml, encoded)
        .with_context(|| format!("failed to write {}", paths.profile_toml.display()))
}

pub fn selected_profile_name() -> Result<Option<String>> {
    let path = active_profile_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .trim()
        .to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

pub fn select_profile(name: &str) -> Result<()> {
    let path = active_profile_path()?;
    let root = config_root()?;
    fs::create_dir_all(&root).with_context(|| format!("failed to create {}", root.display()))?;
    fs::write(&path, name)
        .with_context(|| format!("failed to write selected profile at {}", path.display()))
}

pub fn remove_profile(name: &str) -> Result<()> {
    let paths = profile_paths(name)?;
    if paths.root.exists() {
        fs::remove_dir_all(&paths.root)
            .with_context(|| format!("failed to remove {}", paths.root.display()))?;
    }
    Ok(())
}

pub fn load_reticulum_config(name: &str) -> Result<ReticulumConfig> {
    let path = profile_paths(name)?.reticulum_toml;
    if !path.exists() {
        return Ok(ReticulumConfig::default());
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("failed to read reticulum config {}", path.display()))?;
    toml::from_str(&contents)
        .with_context(|| format!("invalid reticulum config in {}", path.display()))
}

pub fn save_reticulum_config(name: &str, config: &ReticulumConfig) -> Result<()> {
    let path = profile_paths(name)?.reticulum_toml;
    let encoded = toml::to_string_pretty(config).context("failed to encode reticulum config")?;
    fs::write(&path, encoded)
        .with_context(|| format!("failed to write reticulum config {}", path.display()))
}

pub fn resolve_identity_path(settings: &ProfileSettings, paths: &ProfilePaths) -> PathBuf {
    settings
        .identity_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.identity_file.clone())
}

pub fn import_identity(src: &Path, profile_name: &str) -> Result<PathBuf> {
    let paths = profile_paths(profile_name)?;
    fs::create_dir_all(&paths.root)
        .with_context(|| format!("failed to create {}", paths.root.display()))?;
    fs::copy(src, &paths.identity_file).with_context(|| {
        format!(
            "failed to import identity {} -> {}",
            src.display(),
            paths.identity_file.display()
        )
    })?;
    Ok(paths.identity_file)
}

pub fn export_identity(dst: &Path, profile_name: &str) -> Result<PathBuf> {
    let paths = profile_paths(profile_name)?;
    fs::copy(&paths.identity_file, dst).with_context(|| {
        format!(
            "failed to export identity {} -> {}",
            paths.identity_file.display(),
            dst.display()
        )
    })?;
    Ok(dst.to_path_buf())
}

pub fn upsert_interface(config: &mut ReticulumConfig, entry: InterfaceEntry) {
    if let Some(current) = config.interfaces.iter_mut().find(|iface| iface.name == entry.name) {
        *current = entry;
        return;
    }
    config.interfaces.push(entry);
    config.interfaces.sort_by(|a, b| a.name.cmp(&b.name));
}

pub fn set_interface_enabled(config: &mut ReticulumConfig, name: &str, enabled: bool) -> bool {
    if let Some(iface) = config.interfaces.iter_mut().find(|iface| iface.name == name) {
        iface.enabled = enabled;
        return true;
    }
    false
}

pub fn remove_interface(config: &mut ReticulumConfig, name: &str) -> bool {
    let len_before = config.interfaces.len();
    config.interfaces.retain(|iface| iface.name != name);
    len_before != config.interfaces.len()
}
