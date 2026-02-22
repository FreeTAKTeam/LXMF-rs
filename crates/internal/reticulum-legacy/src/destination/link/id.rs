impl From<&Packet> for LinkId {
    fn from(packet: &Packet) -> Self {
        let data = packet.data.as_slice();
        let data_diff =
            if data.len() > PUBLIC_KEY_LENGTH * 2 { data.len() - PUBLIC_KEY_LENGTH * 2 } else { 0 };

        let hashable_data = &data[..data.len() - data_diff];

        AddressHash::new_from_hash(&Hash::new(
            Hash::generator()
                .chain_update([packet.header.to_meta() & 0b00001111])
                .chain_update(packet.destination.as_slice())
                .chain_update([packet.context as u8])
                .chain_update(hashable_data)
                .finalize()
                .into(),
        ))
    }
}
