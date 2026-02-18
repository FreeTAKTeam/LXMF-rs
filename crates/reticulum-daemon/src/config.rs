use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct DaemonConfig {
    #[serde(default)]
    pub interfaces: Vec<InterfaceConfig>,
}

#[derive(Debug, Deserialize)]
pub struct InterfaceConfig {
    #[serde(rename = "type")]
    pub kind: String,
    pub enabled: Option<bool>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub name: Option<String>,
}

impl DaemonConfig {
    pub fn from_toml(input: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(input)
    }

    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let contents = fs::read_to_string(path)?;
        Self::from_toml(&contents)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    }

    pub fn enabled_tcp_clients(&self) -> Vec<&InterfaceConfig> {
        self.interfaces
            .iter()
            .filter(|iface| iface.enabled.unwrap_or(false) && iface.kind == "tcp_client")
            .collect()
    }

    pub fn tcp_client_endpoints(&self) -> Vec<(String, u16)> {
        self.enabled_tcp_clients()
            .iter()
            .filter_map(|iface| {
                let host = iface.host.as_ref()?;
                let port = iface.port?;
                Some((host.clone(), port))
            })
            .collect()
    }
}
