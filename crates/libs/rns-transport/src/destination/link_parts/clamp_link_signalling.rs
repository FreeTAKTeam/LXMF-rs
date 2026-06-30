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
