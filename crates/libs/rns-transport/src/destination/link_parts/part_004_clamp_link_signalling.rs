fn clamp_link_signalling(bytes: [u8; LINK_MTU_SIZE]) -> [u8; LINK_MTU_SIZE] {
    let value = ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | bytes[2] as u32;
    let mode = value & LINK_MODE_MASK;
    let mtu = (value & LINK_MTU_MASK).min(RETICULUM_COMPAT_MTU);
    let value = mode | mtu;
    [((value >> 16) & 0xFF) as u8, ((value >> 8) & 0xFF) as u8, (value & 0xFF) as u8]
}

fn link_close_line(id: &AddressHash) -> String {
    format!("link: close {id}")
}
