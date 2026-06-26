use std::sync::OnceLock;

use crate::packet::PacketType;

const DEFAULT_IFACE_TX_QUEUE_CAPACITY: usize = 128;
const IFACE_TX_ENQUEUE_TIMEOUT_MS: u64 = 200;
const DEFAULT_IFACE_BITRATE_BPS: u64 = 62_500;
const DEFAULT_ANNOUNCE_CAP_PERCENT: u64 = 2;
const DEFAULT_IFACE_MTU: usize =
    crate::packet::PACKET_MDU + 2 + 1 + crate::hash::ADDRESS_HASH_SIZE * 2 + 1;
const MAX_QUEUED_ANNOUNCES_PER_IFACE: usize = 16_384;
const QUEUED_ANNOUNCE_LIFE: Duration = Duration::from_secs(60 * 60 * 24);
const OUTGOING_PR_FREQ_SAMPLES: usize = 48;
const OUTGOING_PR_MIN_LIMIT_SAMPLES: usize = 6;
const OUTGOING_PR_FREQ_DECAY: Duration = Duration::from_secs(10);
const DEFAULT_EGRESS_PR_FREQ_HZ: f64 = 5.0;

fn tx_diag_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("RETICULUMD_DIAGNOSTICS")
            .or_else(|_| std::env::var("RETICULUM_TRANSPORT_DIAGNOSTICS"))
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on" | "debug"
                )
            })
            .unwrap_or(false)
    })
}

fn allows_announce_broadcast(
    packet: &Packet,
    outgoing_mode: InterfaceMode,
    policy: Option<AnnounceBroadcastPolicy>,
) -> bool {
    if packet.header.packet_type != PacketType::Announce {
        return true;
    }

    let Some(policy) = policy else {
        return true;
    };

    match outgoing_mode {
        InterfaceMode::AccessPoint => false,
        InterfaceMode::Roaming => {
            policy.local_destination
                || matches!(
                    policy.next_hop_iface_mode,
                    Some(
                        InterfaceMode::Full
                            | InterfaceMode::PointToPoint
                            | InterfaceMode::AccessPoint
                            | InterfaceMode::Gateway
                    )
                )
        }
        InterfaceMode::Boundary => {
            policy.local_destination
                || matches!(
                    policy.next_hop_iface_mode,
                    Some(
                        InterfaceMode::Full
                            | InterfaceMode::PointToPoint
                            | InterfaceMode::AccessPoint
                            | InterfaceMode::Boundary
                            | InterfaceMode::Gateway
                    )
                )
        }
        InterfaceMode::Full | InterfaceMode::PointToPoint | InterfaceMode::Gateway => true,
    }
}

fn announce_emitted(packet: &Packet) -> u64 {
    let offset = crate::identity::PUBLIC_KEY_LENGTH * 2 + crate::destination::NAME_HASH_LENGTH;
    let end = offset + crate::destination::RAND_HASH_LENGTH;
    let data = packet.data.as_slice();
    if data.len() < end {
        return 0;
    }

    let mut emitted = [0u8; 8];
    emitted[3..].copy_from_slice(&data[offset + 5..end]);
    u64::from_be_bytes(emitted)
}

fn announce_wait(packet: &Packet, bitrate_bps: u64, cap_percent: u64) -> Duration {
    if bitrate_bps == 0 || cap_percent == 0 {
        return Duration::ZERO;
    }

    let bits = packet.to_bytes().map(|raw| raw.len()).unwrap_or(packet.data.len()) as f64 * 8.0;
    let tx_secs = bits / bitrate_bps as f64;
    let wait_secs = tx_secs / (cap_percent as f64 / 100.0);
    Duration::from_secs_f64(wait_secs.max(0.0))
}
