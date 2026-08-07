use super::DiscoveredInterface;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const MONITOR_INTERVAL_SECS: f64 = 5.0;
pub const DETACH_THRESHOLD_SECS: f64 = 12.0;
pub const BLACKHOLE_INITIAL_WAIT_SECS: f64 = 20.0;
pub const BLACKHOLE_JOB_INTERVAL_SECS: f64 = 60.0;
pub const BLACKHOLE_UPDATE_INTERVAL_SECS: f64 = 60.0 * 60.0;

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryAnnouncementCandidate {
    pub id: String,
    pub supports_discovery: bool,
    pub discoverable: bool,
    pub last_announce: f64,
    pub announce_interval: f64,
}

#[derive(Debug, Clone, Default)]
pub struct InterfaceAnnounceScheduler {
    running: bool,
}

impl InterfaceAnnounceScheduler {
    pub fn start(&mut self) -> bool {
        let changed = !self.running;
        self.running = true;
        changed
    }

    pub fn stop(&mut self) -> bool {
        let changed = self.running;
        self.running = false;
        changed
    }

    pub fn next_due<'a>(
        &self,
        interfaces: &'a [DiscoveryAnnouncementCandidate],
        now: f64,
    ) -> Option<&'a DiscoveryAnnouncementCandidate> {
        if !self.running {
            return None;
        }
        interfaces
            .iter()
            .filter(|interface| {
                interface.supports_discovery
                    && interface.discoverable
                    && now > interface.last_announce + interface.announce_interval
            })
            .max_by(|left, right| {
                (now - left.last_announce)
                    .partial_cmp(&(now - right.last_announce))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInterfaceState {
    pub id: String,
    pub autoconnect_hash: Option<[u8; 32]>,
    pub autoconnect_source: Option<String>,
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
    pub i2p_b32: Option<String>,
    pub bootstrap_only: bool,
    pub online: bool,
    pub down_since: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoconnectPlan {
    pub endpoint_hash: [u8; 32],
    pub name: String,
    pub target_host: String,
    pub target_port: u16,
    pub transport_identity: String,
    pub network_id: String,
    pub ifac_netname: Option<String>,
    pub ifac_netkey: Option<String>,
    /// Reticulum 1.4.2 policy applied to the synthesized interface.
    pub interface_mode: Option<String>,
    pub gravity: i64,
    pub announces_to_internal: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AutoconnectInterfacePolicy<'a> {
    pub mode: Option<&'a str>,
    pub gravity: i64,
    pub announces_to_internal: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryMonitorPlan {
    pub recovered: Vec<String>,
    pub marked_down: Vec<String>,
    pub detach: Vec<String>,
    pub enable_bootstrap: bool,
    pub disable_bootstrap: Vec<String>,
    pub free_slots: usize,
    pub reserved_slots: usize,
}

#[derive(Debug, Clone, Default)]
pub struct InterfaceDiscoveryLifecycle {
    running: bool,
    initial_autoconnect_ran: bool,
    monitored: BTreeSet<String>,
}

impl InterfaceDiscoveryLifecycle {
    pub fn start(&mut self) -> bool {
        let changed = !self.running;
        self.running = true;
        changed
    }

    pub fn stop(&mut self) -> bool {
        let changed = self.running;
        self.running = false;
        changed
    }

    pub fn monitor_interface(&mut self, id: impl Into<String>) -> bool {
        self.monitored.insert(id.into())
    }

    pub fn teardown_interface(&mut self, id: &str) -> bool {
        self.monitored.remove(id)
    }

    pub fn initial_autoconnect(
        &mut self,
        discovered: &[DiscoveredInterface],
        existing: &[RuntimeInterfaceState],
        maximum: usize,
    ) -> Vec<AutoconnectPlan> {
        let mut simulated = existing.to_vec();
        let mut plans = Vec::new();
        for info in discovered {
            if autoconnect_count(&simulated) >= maximum {
                break;
            }
            if let Some(plan) = plan_autoconnect(info, &simulated) {
                simulated.push(RuntimeInterfaceState {
                    id: info.name.clone(),
                    autoconnect_hash: Some(plan.endpoint_hash),
                    autoconnect_source: Some(plan.network_id.clone()),
                    target_host: Some(plan.target_host.clone()),
                    target_port: Some(plan.target_port),
                    i2p_b32: None,
                    bootstrap_only: false,
                    online: false,
                    down_since: None,
                });
                plans.push(plan);
            }
        }
        self.initial_autoconnect_ran = true;
        plans
    }

    pub fn monitor(
        &self,
        now_secs: u64,
        interfaces: &mut [RuntimeInterfaceState],
        maximum: usize,
    ) -> DiscoveryMonitorPlan {
        let mut plan = DiscoveryMonitorPlan::default();
        let mut online_autoconnects = 0;
        for interface in interfaces
            .iter_mut()
            .filter(|iface| iface.autoconnect_hash.is_some() && self.monitored.contains(&iface.id))
        {
            if interface.online {
                online_autoconnects += 1;
                if interface.down_since.take().is_some() {
                    plan.recovered.push(interface.id.clone());
                }
            } else if let Some(down_since) = interface.down_since {
                if now_secs.saturating_sub(down_since) >= DETACH_THRESHOLD_SECS as u64 {
                    plan.detach.push(interface.id.clone());
                }
            } else {
                interface.down_since = Some(now_secs);
                plan.marked_down.push(interface.id.clone());
            }
        }
        let connected = autoconnect_count(interfaces);
        plan.free_slots = maximum.saturating_sub(connected);
        plan.reserved_slots = maximum / 4;
        if online_autoconnects >= maximum {
            plan.disable_bootstrap.extend(
                interfaces
                    .iter()
                    .filter(|interface| interface.bootstrap_only)
                    .map(|interface| interface.id.clone()),
            );
        }
        plan.enable_bootstrap =
            online_autoconnects == 0 && bootstrap_interface_count(interfaces) == 0;
        plan
    }

    pub const fn initial_autoconnect_ran(&self) -> bool {
        self.initial_autoconnect_ran
    }
}

pub fn endpoint_hash(info: &DiscoveredInterface) -> [u8; 32] {
    let mut endpoint = info.reachable_on.clone().unwrap_or_default();
    if let Some(port) = info.port {
        endpoint.push(':');
        endpoint.push_str(&port.to_string());
    }
    Sha256::digest(endpoint.as_bytes()).into()
}

pub fn interface_exists(info: &DiscoveredInterface, interfaces: &[RuntimeInterfaceState]) -> bool {
    let hash = endpoint_hash(info);
    interfaces.iter().any(|interface| {
        interface.autoconnect_hash == Some(hash)
            || (interface.target_host == info.reachable_on
                && (info.port.is_none() || interface.target_port == info.port))
            || (interface.i2p_b32.is_some() && interface.i2p_b32 == info.reachable_on)
    })
}

pub fn autoconnect_count(interfaces: &[RuntimeInterfaceState]) -> usize {
    interfaces.iter().filter(|interface| interface.autoconnect_hash.is_some()).count()
}

pub fn bootstrap_interface_count(interfaces: &[RuntimeInterfaceState]) -> usize {
    interfaces.iter().filter(|interface| interface.bootstrap_only).count()
}

pub fn plan_autoconnect(
    info: &DiscoveredInterface,
    existing: &[RuntimeInterfaceState],
) -> Option<AutoconnectPlan> {
    plan_autoconnect_with_policy(info, existing, AutoconnectInterfacePolicy::default())
}

pub fn plan_autoconnect_with_policy(
    info: &DiscoveredInterface,
    existing: &[RuntimeInterfaceState],
    policy: AutoconnectInterfacePolicy<'_>,
) -> Option<AutoconnectPlan> {
    if !matches!(info.interface_type.as_str(), "BackboneInterface" | "TCPServerInterface")
        || interface_exists(info, existing)
    {
        return None;
    }
    Some(AutoconnectPlan {
        endpoint_hash: endpoint_hash(info),
        name: info.name.clone(),
        target_host: info.reachable_on.clone()?,
        target_port: info.port?,
        transport_identity: info.transport_id.clone(),
        network_id: info.network_id.clone(),
        ifac_netname: info.ifac_netname.clone(),
        ifac_netkey: info.ifac_netkey.clone(),
        interface_mode: policy.mode.map(str::to_owned),
        gravity: policy.gravity,
        announces_to_internal: policy.announces_to_internal,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackholeEntry {
    pub source: String,
    pub until: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BlackholeUpdateScheduler {
    running: bool,
    started_at: u64,
    last_updates: BTreeMap<String, u64>,
}

impl BlackholeUpdateScheduler {
    pub fn start(&mut self, now_secs: u64) -> bool {
        let changed = !self.running;
        if changed {
            self.started_at = now_secs;
        }
        self.running = true;
        changed
    }

    pub fn stop(&mut self) -> bool {
        let changed = self.running;
        self.running = false;
        changed
    }

    pub fn due_sources(&self, sources: &[String], now_secs: u64) -> Vec<String> {
        if !self.running
            || now_secs < self.started_at.saturating_add(BLACKHOLE_INITIAL_WAIT_SECS as u64)
        {
            return Vec::new();
        }
        sources
            .iter()
            .filter(|source| {
                now_secs
                    > self
                        .last_updates
                        .get(*source)
                        .copied()
                        .unwrap_or_default()
                        .saturating_add(BLACKHOLE_UPDATE_INTERVAL_SECS as u64)
            })
            .cloned()
            .collect()
    }

    pub fn mark_updated(&mut self, source: impl Into<String>, now_secs: u64) {
        self.last_updates.insert(source.into(), now_secs);
    }
}

pub fn merge_blackhole_update(
    current: &mut BTreeMap<String, BlackholeEntry>,
    update: BTreeMap<String, BlackholeEntry>,
) -> usize {
    let mut added = 0;
    for (identity, entry) in update {
        if let std::collections::btree_map::Entry::Vacant(slot) = current.entry(identity) {
            slot.insert(entry);
            added += 1;
        }
    }
    added
}

pub fn persist_blackhole_source(
    directory: impl AsRef<Path>,
    source: &str,
    entries: &BTreeMap<String, BlackholeEntry>,
) -> io::Result<PathBuf> {
    fs::create_dir_all(directory.as_ref())?;
    let path = directory.as_ref().join(source);
    let temporary = directory.as_ref().join(format!("{source}.tmp"));
    let payload = rmp_serde::to_vec_named(entries)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(&temporary, payload)?;
    fs::rename(&temporary, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    include!("lifecycle_tests.rs");
}
