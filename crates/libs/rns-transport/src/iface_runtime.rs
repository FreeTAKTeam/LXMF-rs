use crate::packet::PacketType;

const DEFAULT_IFACE_TX_QUEUE_CAPACITY: usize = 128;
const IFACE_TX_ENQUEUE_TIMEOUT_MS: u64 = 200;
const DEFAULT_IFACE_BITRATE_BPS: u64 = 62_500;
const DEFAULT_ANNOUNCE_CAP_PERCENT: u64 = 2;
const DEFAULT_IFACE_MTU: usize =
    crate::packet::PACKET_MDU + 2 + 1 + crate::hash::ADDRESS_HASH_SIZE * 2 + 1;
const MAX_QUEUED_ANNOUNCES_PER_IFACE: usize = 4_096;
const QUEUED_ANNOUNCE_LIFE: Duration = Duration::from_secs(60 * 60 * 24);
const OUTGOING_PR_FREQ_SAMPLES: usize = 48;
const OUTGOING_PR_MIN_LIMIT_SAMPLES: usize = 6;
const OUTGOING_PR_FREQ_DECAY: Duration = Duration::from_secs(10);
const DEFAULT_EGRESS_PR_FREQ_HZ: f64 = 5.0;

fn allows_announce_broadcast(
    packet: &Packet,
    outgoing_mode: InterfaceMode,
    outgoing_announces_from_internal: Option<bool>,
    policy: Option<AnnounceBroadcastPolicy>,
) -> bool {
    if packet.header.packet_type != PacketType::Announce {
        return true;
    }

    let Some(policy) = policy else {
        return true;
    };

    // Reference parity: `Transport.py`'s per-interface announce ladder opens
    // with a rung that is not mode-specific — an announce for a destination
    // this node does not own, with no next-hop interface on file, is blocked
    // on every outgoing interface ("Blocking announce broadcast on <iface>
    // since next hop interface doesn't exist"). Only `Roaming` and `Boundary`
    // block a `None` next hop below, and they do it as a side effect of
    // matching `Some(..)`, so the other four modes let it through today.
    if !policy.local_destination && policy.next_hop_iface_mode.is_none() {
        return false;
    }

    // The reference's next rung, ahead of every mode-specific one — including
    // AP, which is why this cannot be folded into the `match` below:
    //
    //     elif (not local_destination
    //           and interface.announces_from_internal == False
    //           and from_interface.mode == MODE_INTERNAL):
    //
    // An interface that opts out refuses to carry anything learned over an
    // internal link. Default is `True`, so opting out is explicit.
    if !policy.local_destination
        && outgoing_announces_from_internal == Some(false)
        && policy.next_hop_iface_mode == Some(InterfaceMode::Internal)
    {
        return false;
    }

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
                            | InterfaceMode::Internal
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
                            | InterfaceMode::Internal
                    )
                )
        }
        // The reference's `MODE_INTERNAL` rung. An internal interface joins
        // this node to another instance of itself, so it carries everything
        // except an announce that reached us over a boundary — a boundary
        // marks the edge of a local topology, and carrying across it into a
        // shared instance would import the far side's announces. The next
        // hop's own `announces_to_internal` overrides that block:
        //
        //     if from_interface.announces_to_internal == True: pass
        //     elif from_interface.mode == MODE_BOUNDARY: should_transmit = False
        InterfaceMode::Internal => {
            policy.local_destination
                || policy.next_hop_announces_to_internal == Some(true)
                || policy.next_hop_iface_mode != Some(InterfaceMode::Boundary)
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
