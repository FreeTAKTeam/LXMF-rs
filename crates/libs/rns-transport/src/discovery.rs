use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

pub mod announce;
pub mod lifecycle;

pub const THRESHOLD_UNKNOWN_SECS: f64 = 24.0 * 60.0 * 60.0;
pub const THRESHOLD_STALE_SECS: f64 = 3.0 * 24.0 * 60.0 * 60.0;
pub const THRESHOLD_REMOVE_SECS: f64 = 7.0 * 24.0 * 60.0 * 60.0;

const DISCOVERABLE_TYPES: &[&str] = &[
    "BackboneInterface",
    "TCPServerInterface",
    "TCPClientInterface",
    "I2PInterface",
    "RNodeInterface",
    "WeaveInterface",
    "KISSInterface",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoveredInterface {
    pub discovery_hash: Vec<u8>,
    #[serde(rename = "type")]
    pub interface_type: String,
    pub transport: bool,
    pub name: String,
    pub received: f64,
    #[serde(default)]
    pub stamp: Vec<u8>,
    pub value: u64,
    pub transport_id: String,
    pub network_id: String,
    pub hops: u8,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub height: Option<f64>,
    #[serde(default)]
    pub operator_lxmf_address: Option<String>,
    #[serde(default)]
    pub reachable_on: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub ifac_netname: Option<String>,
    #[serde(default)]
    pub ifac_netkey: Option<String>,
    #[serde(default)]
    pub config_entry: Option<String>,
    #[serde(default)]
    pub discovered: f64,
    #[serde(default)]
    pub last_heard: f64,
    #[serde(default)]
    pub heard_count: u64,
    #[serde(default)]
    pub status: DiscoveryStatus,
    #[serde(default)]
    pub status_code: u16,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiscoveryStatus {
    Stale,
    Unknown,
    #[default]
    Available,
}

impl DiscoveryStatus {
    pub const fn code(self) -> u16 {
        match self {
            Self::Stale => 0,
            Self::Unknown => 100,
            Self::Available => 1000,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DiscoveryListFilter {
    pub only_available: bool,
    pub only_transport: bool,
}

#[derive(Debug, Clone)]
pub struct InterfaceDiscoveryStore {
    directory: PathBuf,
}

impl InterfaceDiscoveryStore {
    pub fn new(reticulum_storage_path: impl AsRef<Path>) -> Self {
        Self { directory: reticulum_storage_path.as_ref().join("discovery/interfaces") }
    }

    pub fn observe(&self, mut info: DiscoveredInterface) -> io::Result<DiscoveredInterface> {
        validate_record(&info)?;
        fs::create_dir_all(&self.directory)?;
        let path = self.record_path(&info.discovery_hash);
        if let Ok(previous) = read_record(&path) {
            info.discovered = previous.discovered;
            info.heard_count = previous.heard_count.saturating_add(1);
        } else {
            info.discovered = info.received;
            info.heard_count = 0;
        }
        info.last_heard = info.received;
        write_record(&path, &info)?;
        Ok(info)
    }

    pub fn list(
        &self,
        now: f64,
        allowed_network_ids: &[String],
        filter: DiscoveryListFilter,
    ) -> io::Result<Vec<DiscoveredInterface>> {
        if !self.directory.exists() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }
            let mut info = match read_record(&path) {
                Ok(info) => info,
                Err(error) => {
                    log::warn!(
                        "ignoring malformed discovered interface {}: {error}",
                        path.display()
                    );
                    continue;
                }
            };
            let heard_delta = (now - info.last_heard).max(0.0);
            let source_allowed = allowed_network_ids.is_empty()
                || allowed_network_ids.iter().any(|source| source == &info.network_id);
            if heard_delta > THRESHOLD_REMOVE_SECS
                || !source_allowed
                || validate_record(&info).is_err()
            {
                if let Err(error) = fs::remove_file(&path) {
                    log::warn!(
                        "failed to remove invalid discovery record {}: {error}",
                        path.display()
                    );
                }
                continue;
            }
            info.status = if heard_delta > THRESHOLD_STALE_SECS {
                DiscoveryStatus::Stale
            } else if heard_delta > THRESHOLD_UNKNOWN_SECS {
                DiscoveryStatus::Unknown
            } else {
                DiscoveryStatus::Available
            };
            info.status_code = info.status.code();
            if filter.only_available && info.status != DiscoveryStatus::Available {
                continue;
            }
            if filter.only_transport && !info.transport {
                continue;
            }
            records.push(info);
        }
        records.sort_by(|left, right| {
            right
                .status_code
                .cmp(&left.status_code)
                .then_with(|| right.value.cmp(&left.value))
                .then_with(|| {
                    right.last_heard.partial_cmp(&left.last_heard).unwrap_or(Ordering::Equal)
                })
        });
        Ok(records)
    }

    fn record_path(&self, discovery_hash: &[u8]) -> PathBuf {
        self.directory.join(hex::encode(discovery_hash))
    }
}

fn validate_record(info: &DiscoveredInterface) -> io::Result<()> {
    if !DISCOVERABLE_TYPES.contains(&info.interface_type.as_str()) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported interface type"));
    }
    if let Some(endpoint) = &info.reachable_on {
        if !valid_endpoint(endpoint) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid reachable_on"));
        }
    }
    Ok(())
}

pub fn is_ip_address(endpoint: &str) -> bool {
    endpoint.parse::<IpAddr>().is_ok()
}

pub fn is_invalid_ip_address(endpoint: &str) -> bool {
    matches!(endpoint, "127.0.0.1" | "0.0.0.0")
}

pub fn is_onion_address(endpoint: &str) -> bool {
    endpoint.to_ascii_lowercase().ends_with(".onion")
}

pub fn is_hostname(endpoint: &str) -> bool {
    let endpoint = endpoint.trim_end_matches('.');
    if endpoint
        .rsplit('.')
        .next()
        .is_some_and(|label| label.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return false;
    }
    !endpoint.is_empty()
        && endpoint.len() <= 253
        && endpoint.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn valid_endpoint(endpoint: &str) -> bool {
    if is_ip_address(endpoint) {
        return true;
    }
    is_hostname(endpoint)
}

fn read_record(path: &Path) -> io::Result<DiscoveredInterface> {
    let payload = fs::read(path)?;
    rmp_serde::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn write_record(path: &Path, info: &DiscoveredInterface) -> io::Result<()> {
    let payload = rmp_serde::to_vec_named(info)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(path, payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rns_1_5_discovery_endpoint_helpers_match_python() {
        assert!(is_invalid_ip_address("127.0.0.1"));
        assert!(is_invalid_ip_address("0.0.0.0"));
        assert!(!is_invalid_ip_address("127.0.0.2"));
        assert!(is_onion_address("EXAMPLE.ONION"));
        assert!(!is_onion_address("example.onion.test"));
    }

    fn record(hash: u8, received: f64) -> DiscoveredInterface {
        DiscoveredInterface {
            discovery_hash: vec![hash; 32],
            interface_type: "BackboneInterface".to_string(),
            transport: true,
            name: format!("peer-{hash}"),
            received,
            stamp: vec![hash; 32],
            value: u64::from(hash),
            transport_id: hex::encode([hash; 16]),
            network_id: hex::encode([0x22; 16]),
            hops: 1,
            latitude: None,
            longitude: None,
            height: None,
            operator_lxmf_address: None,
            reachable_on: Some("localhost".to_string()),
            port: Some(4242),
            ifac_netname: None,
            ifac_netkey: None,
            config_entry: None,
            discovered: 0.0,
            last_heard: 0.0,
            heard_count: 0,
            status: DiscoveryStatus::Available,
            status_code: 0,
        }
    }

    #[test]
    fn persistence_ages_filters_sorts_and_expires_like_python() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = InterfaceDiscoveryStore::new(temp.path());
        let now = 1_000_000.0;
        store.observe(record(1, now - 10.0)).expect("available");
        store.observe(record(3, now - THRESHOLD_UNKNOWN_SECS - 1.0)).expect("unknown");
        store.observe(record(2, now - THRESHOLD_STALE_SECS - 1.0)).expect("stale");
        store.observe(record(4, now - THRESHOLD_REMOVE_SECS - 1.0)).expect("expired");

        let rows = store.list(now, &[], DiscoveryListFilter::default()).expect("list");
        assert_eq!(rows.iter().map(|row| row.discovery_hash[0]).collect::<Vec<_>>(), [1, 3, 2]);
        assert_eq!(rows[0].status, DiscoveryStatus::Available);
        assert_eq!(rows[1].status, DiscoveryStatus::Unknown);
        assert_eq!(rows[2].status, DiscoveryStatus::Stale);

        let available = store
            .list(now, &[], DiscoveryListFilter { only_available: true, only_transport: true })
            .expect("available");
        assert_eq!(available.len(), 1);
    }

    #[test]
    fn repeated_observation_preserves_first_seen_and_increments_heard_count() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = InterfaceDiscoveryStore::new(temp.path());
        store.observe(record(1, 10.0)).expect("first");
        let updated = store.observe(record(1, 20.0)).expect("second");
        assert_eq!(updated.discovered, 10.0);
        assert_eq!(updated.last_heard, 20.0);
        assert_eq!(updated.heard_count, 1);
    }
}
