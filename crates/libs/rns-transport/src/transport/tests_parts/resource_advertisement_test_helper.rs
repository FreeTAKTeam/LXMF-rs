fn decrypt_resource_advertisement(link: &Link, packet: &Packet) -> ResourceAdvertisement {
    let mut buffer = PacketDataBuffer::new();
    let plain_len = {
        let plain = link
            .decrypt(packet.data.as_slice(), buffer.accuire_buf_max())
            .expect("decrypt resource advertisement");
        plain.len()
    };
    buffer.resize(plain_len);
    ResourceAdvertisement::unpack(buffer.as_slice()).expect("resource advertisement")
}
