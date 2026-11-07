use super::super::snapshot_allocation_tests::{path_entry, tunnel_entry};
use super::*;
use rmpv::Value as RmpValue;

// Pre-change encoders are independent byte-for-byte oracles, not round-trip
// checks that could hide a matching encoder/decoder error.
fn legacy_paths(entries: &[PythonPathEntry]) -> Result<Vec<u8>, RnsError> {
    let value = RmpValue::Array(
        entries
            .iter()
            .map(|entry| {
                RmpValue::Array(vec![
                    RmpValue::Binary(entry.destination.as_slice().to_vec()),
                    RmpValue::F64(entry.timestamp_secs),
                    RmpValue::Binary(entry.received_from.as_slice().to_vec()),
                    RmpValue::from(u64::from(entry.hops)),
                    RmpValue::F64(entry.expires_secs),
                    RmpValue::Array(
                        entry
                            .random_blobs
                            .iter()
                            .skip(entry.random_blobs.len().saturating_sub(MAX_RANDOM_BLOBS))
                            .map(|blob| RmpValue::Binary(blob.to_vec()))
                            .collect(),
                    ),
                    RmpValue::Binary(entry.interface_hash.as_slice().to_vec()),
                    RmpValue::Binary(entry.packet_hash.as_slice().to_vec()),
                ])
            })
            .collect(),
    );
    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, &value).map_err(|_| RnsError::InvalidArgument)?;
    Ok(out)
}

fn legacy_tunnels(entries: &[PythonTunnelEntry]) -> Result<Vec<u8>, RnsError> {
    let value = RmpValue::Array(
        entries
            .iter()
            .map(|entry| {
                RmpValue::Array(vec![
                    RmpValue::Binary(entry.tunnel_id.as_slice().to_vec()),
                    optional_hash_value(entry.interface_hash),
                    RmpValue::Array(
                        entry
                            .paths
                            .iter()
                            .map(|path| {
                                RmpValue::Array(vec![
                                    RmpValue::Binary(path.destination.as_slice().to_vec()),
                                    RmpValue::F64(path.timestamp_secs),
                                    RmpValue::Binary(path.received_from.as_slice().to_vec()),
                                    RmpValue::from(u64::from(path.hops)),
                                    RmpValue::F64(path.expires_secs),
                                    RmpValue::Array(
                                        path.random_blobs
                                            .iter()
                                            .map(|blob| RmpValue::Binary(blob.to_vec()))
                                            .collect(),
                                    ),
                                    optional_hash_value(path.interface_hash),
                                    RmpValue::Binary(path.packet_hash.as_slice().to_vec()),
                                ])
                            })
                            .collect(),
                    ),
                    RmpValue::F64(entry.expires_secs),
                ])
            })
            .collect(),
    );
    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, &value).map_err(|_| RnsError::InvalidArgument)?;
    Ok(out)
}

fn optional_hash_value(hash: Option<Hash>) -> RmpValue {
    hash.map(|hash| RmpValue::Binary(hash.as_slice().to_vec())).unwrap_or(RmpValue::Nil)
}

#[test]
fn path_snapshot_bytes_match_legacy_binary_float_integer_and_blob_encodings() {
    for len in [0, 1, 15, 16, 255, 256] {
        let entries = (0..len)
            .map(|index| {
                let mut entry = path_entry(index, [0, 1, 64, 65, 70][index % 5]);
                for (blob_index, blob) in entry.random_blobs.iter_mut().enumerate() {
                    blob[0] = blob_index as u8;
                }
                entry.hops = [0, 127, 128, 255][index % 4];
                entry.timestamp_secs =
                    [0.0, -0.0, 1.5, f64::from_bits(0x7ff8_0000_0000_1234)][index % 4];
                entry.expires_secs = f64::INFINITY;
                entry
            })
            .collect::<Vec<_>>();
        assert_eq!(
            encode_paths(&entries).expect("direct"),
            legacy_paths(&entries).expect("legacy")
        );
    }
}

#[test]
fn tunnel_snapshot_bytes_preserve_optional_hashes_and_untruncated_raw_blobs() {
    for len in [0, 1, 15, 16] {
        let entries = (0..len)
            .map(|index| {
                let mut entry = tunnel_entry(index, [0, 1, 64, 65, 70][index % 5]);
                entry.paths[0].hops = [0, 127, 128, 255][index % 4];
                entry.paths[0].timestamp_secs = -0.0;
                entry.paths[0].expires_secs = f64::from_bits(0x7ff8_0000_0000_1234);
                entry.paths[0].interface_hash = (index % 2 == 1).then_some(entry.tunnel_id);
                for (blob_index, blob) in entry.paths[0].random_blobs.iter_mut().enumerate() {
                    blob[0] = blob_index as u8;
                }
                if index % 7 == 0 {
                    entry.paths.clear();
                }
                entry
            })
            .collect::<Vec<_>>();
        assert_eq!(
            encode_tunnels(&entries).expect("direct"),
            legacy_tunnels(&entries).expect("legacy")
        );
    }
}

#[test]
fn snapshot_array_headers_use_checked_messagepack_lengths() {
    for (len, expected) in [
        (0, vec![0x90]),
        (15, vec![0x9f]),
        (16, vec![0xdc, 0, 16]),
        (65536, vec![0xdd, 0, 1, 0, 0]),
    ] {
        let mut encoder = Encoder::default();
        encoder.array(len).expect("array header");
        assert_eq!(encoder.0, expected);
    }
    if let Some(overflow) = (u32::MAX as usize).checked_add(1) {
        assert!(Encoder::default().array(overflow).is_err());
    }
}

#[test]
fn nested_tunnel_path_array_preserves_order_and_mixed_interfaces() {
    let mut tunnel = tunnel_entry(90, 0);
    tunnel.paths = (0..16)
        .map(|index| {
            let mut path = tunnel_entry(index, [0, 1, 64, 65, 70][index % 5])
                .paths
                .pop()
                .expect("single seeded path");
            path.interface_hash = index.is_multiple_of(2).then_some(tunnel.tunnel_id);
            path
        })
        .collect();
    let entries = [tunnel];
    assert_eq!(
        encode_tunnels(&entries).expect("direct"),
        legacy_tunnels(&entries).expect("legacy")
    );
}
