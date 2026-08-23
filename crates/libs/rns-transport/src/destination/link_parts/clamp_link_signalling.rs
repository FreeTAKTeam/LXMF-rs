/// The MTU a peer signalled, decoded back out of the stored suffix.
///
/// The signalling bytes are kept verbatim (already clamped on receipt) so
/// that the negotiated value can be recovered here rather than being
/// recomputed from whatever the local interface happens to be. Those are
/// different numbers: the local interface bounds what *we* can carry, the
/// negotiated value bounds what the *path* can carry, and it is the latter
/// that must size anything put on the wire.
///
/// `None` means the peer sent no suffix at all — an older peer, or one that
/// never signalled — in which case the legacy Reticulum MTU is the only
/// safe assumption.
pub(crate) fn link_signalled_mtu(signalling: Option<[u8; LINK_MTU_SIZE]>) -> usize {
    let Some(bytes) = signalling else {
        return LEGACY_RETICULUM_MTU;
    };
    let value = ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | bytes[2] as u32;
    let mtu = (value & LINK_MTU_MASK) as usize;
    // Python treats a zero signalling value as the legacy MTU fallback, while a non-zero
    // hardware MTU below 500 remains meaningful for constrained interfaces.
    if mtu == 0 { LEGACY_RETICULUM_MTU } else { mtu }
}

fn clamp_link_signalling(bytes: [u8; LINK_MTU_SIZE]) -> [u8; LINK_MTU_SIZE] {
    let value = ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | bytes[2] as u32;
    let mode = value & LINK_MODE_MASK;
    let mtu = (value & LINK_MTU_MASK).min(RETICULUM_COMPAT_MTU);
    let value = mode | mtu;
    [((value >> 16) & 0xFF) as u8, ((value >> 8) & 0xFF) as u8, (value & 0xFF) as u8]
}

pub(crate) fn clamp_link_request_signalling_mtu(packet: &mut Packet, max_mtu: usize) -> bool {
    let signalling_start = PUBLIC_KEY_LENGTH * 2;
    if packet.data.len() < signalling_start + LINK_MTU_SIZE {
        return false;
    }

    let max_mtu = u32::try_from(max_mtu).unwrap_or(u32::MAX).min(RETICULUM_COMPAT_MTU);
    let data = packet.data.as_mut_slice();
    let value = ((data[signalling_start] as u32) << 16)
        | ((data[signalling_start + 1] as u32) << 8)
        | data[signalling_start + 2] as u32;
    let mode = value & LINK_MODE_MASK;
    let mtu = (value & LINK_MTU_MASK).min(max_mtu);
    let value = mode | mtu;
    data[signalling_start] = ((value >> 16) & 0xFF) as u8;
    data[signalling_start + 1] = ((value >> 8) & 0xFF) as u8;
    data[signalling_start + 2] = (value & 0xFF) as u8;
    true
}

fn link_close_line(id: &AddressHash) -> String {
    format!("link: close {id}")
}
