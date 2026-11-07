#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::link::LinkHandleResult;
    use crate::destination::{DestinationDesc, DestinationName};
    use crate::identity::PrivateIdentity;
    use rand_core::OsRng;

    /// Inter-part arrival interval used by the request tests. Small enough
    /// that `part_timeout`'s grace floor dominates, so a test that wants a
    /// timeout has to advance the clock past it deliberately.
    const TEST_ARRIVAL_INTERVAL: Duration = Duration::from_millis(1);

    include!("tests/transfers.rs");
    include!("tests/admission.rs");

    include!("tests_mtu.rs");
    include!("tests_retry_failures.rs");
    include!("tests_hashmap_gate.rs");
    include!("tests_window_rounds.rs");
    include!("tests_split_assembly.rs");
    include!("tests_split_lazy.rs");
    include!("tests_split_metadata.rs");
    include!("tests_timeouts.rs");
    include!("tests_timeouts_cleanup.rs");
    include!("tests_timeouts_lifecycle.rs");
    include!("tests_collision.rs");
    include!("tests_fault_injection.rs");
    include!("tests_reader.rs");
    include!("tests_completion.rs");
    include!("tests_request_cleanup.rs");

    fn resource_packet(context: PacketContext, payload: &[u8], destination: AddressHash) -> Packet {
        Packet {
            header: Header {
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            destination,
            context,
            data: PacketDataBuffer::new_from_slice(payload),
            ..Default::default()
        }
    }

    fn split_test_segment(
        data: &[u8],
        original_hash: Option<Hash>,
        segment_index: u32,
        total_segments: u32,
        total_data_size: u64,
    ) -> (ResourceAdvertisement, Vec<u8>) {
        let random_hash = segment_index.to_be_bytes();
        let mut part = Vec::with_capacity(RANDOM_HASH_SIZE + data.len());
        part.extend_from_slice(&random_hash);
        part.extend_from_slice(data);
        let resource_hash = Hash::new_from_slice(&[data, random_hash.as_slice()].concat());
        let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
        hashmap.extend_from_slice(&map_hash(&part, &random_hash));
        (
            ResourceAdvertisement {
                transfer_size: part.len() as u64,
                data_size: total_data_size,
                parts: 1,
                hash: resource_hash,
                random_hash,
                original_hash: original_hash.unwrap_or(resource_hash),
                segment_index,
                total_segments,
                request_id: None,
                flags: FLAG_SPLIT,
                hashmap,
            },
            part,
        )
    }

    fn decrypt_advertisement(link: &Link, packet: &Packet) -> ResourceAdvertisement {
        let mut buffer = PacketDataBuffer::new();
        let plain_len = {
            let plain = link
                .decrypt(packet.data.as_slice(), buffer.accuire_buf_max())
                .expect("decrypt adv");
            plain.len()
        };
        buffer.resize(plain_len);
        ResourceAdvertisement::unpack(buffer.as_slice()).expect("advertisement")
    }
}
