use super::{AddressHash, NextHopMetrics, Transport};
use std::time::Duration;

pub(super) fn medium_path_timeout_for_bitrate(bitrate: Option<u64>) -> Duration {
    Duration::from_secs_f64(medium_path_timeout_seconds_for_bitrate(bitrate))
}

fn medium_path_timeout_seconds_for_bitrate(bitrate: Option<u64>) -> f64 {
    const RETICULUM_MTU_BYTES: f64 = 500.0;
    const MINIMUM_BITRATE: u64 = 5;
    const DEFAULT_PER_HOP_TIMEOUT_SECS: f64 = 6.0;

    let Some(bitrate) = bitrate.filter(|value| *value > 0) else { return 0.0 };
    2.0 * (RETICULUM_MTU_BYTES * 8.0 / bitrate.max(MINIMUM_BITRATE) as f64)
        + DEFAULT_PER_HOP_TIMEOUT_SECS
}

/// Raw measurements for scheduling without rounding through timer durations.
/// Path presence and selected interface come from one path-table read; interface
/// measurements come from one subsequent interface-manager read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathTimingSnapshot {
    pub path_known: bool,
    pub lowest_interface_bitrate: Option<u64>,
    pub next_hop: Option<NextHopMetrics>,
}

impl PathTimingSnapshot {
    pub fn medium_timeout_seconds(self) -> f64 {
        medium_path_timeout_seconds_for_bitrate(self.lowest_interface_bitrate)
    }
}

impl Transport {
    pub async fn path_timing(&self, destination: &AddressHash) -> PathTimingSnapshot {
        let interface = self.handler.lock().await.path_table.next_hop_iface(destination);
        let manager = self.iface_manager.lock().await;
        let next_hop = interface.and_then(|interface| {
            let bitrate = manager.announce_pacing(&interface)?.0;
            Some(NextHopMetrics {
                interface,
                bitrate,
                hardware_mtu: manager.mtu(&interface),
                per_bit_latency: (bitrate > 0).then(|| 1.0 / bitrate as f64),
            })
        });
        PathTimingSnapshot {
            path_known: interface.is_some(),
            lowest_interface_bitrate: manager.lowest_interface_bitrate(),
            next_hop,
        }
    }
}
