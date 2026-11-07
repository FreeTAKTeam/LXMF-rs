//! Run this ignored probe alone in a fresh test process: allocator peak is global.
use crate::destination::RAND_HASH_LENGTH;
use crate::hash::{AddressHash, Hash};
use crate::transport::path_table::{PathTable, PythonPathEntry};
use crate::transport::tunnels::{PythonTunnelEntry, PythonTunnelPathEntry, TunnelTable};
use sha2::{Digest, Sha256};
use std::time::Instant;

// Safe instrumentation only in the library test binary, without an allocation
// limit. The daemon already uses cap; no production allocator is changed.
#[global_allocator]
static ALLOCATOR: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, usize::MAX);

pub(super) fn path_entry(index: usize, blobs: usize) -> PythonPathEntry {
    let hash = Hash::new_from_slice(&index.to_be_bytes());
    PythonPathEntry {
        destination: AddressHash::new_from_hash(&hash),
        timestamp_secs: 1_700_000_000.25,
        received_from: AddressHash::new_from_hash(&hash),
        hops: (index % 256) as u8,
        expires_secs: 1_700_604_800.25,
        random_blobs: vec![[7; RAND_HASH_LENGTH]; blobs],
        iface: AddressHash::new_from_hash(&hash),
        interface_hash: hash,
        packet_hash: hash,
    }
}

pub(super) fn tunnel_entry(index: usize, blobs: usize) -> PythonTunnelEntry {
    let path = path_entry(index, blobs);
    PythonTunnelEntry {
        tunnel_id: path.packet_hash,
        interface_hash: (index.is_multiple_of(2)).then_some(path.interface_hash),
        expires_secs: path.expires_secs,
        paths: vec![PythonTunnelPathEntry {
            destination: path.destination,
            timestamp_secs: path.timestamp_secs,
            received_from: path.received_from,
            hops: path.hops,
            expires_secs: path.expires_secs,
            random_blobs: path.random_blobs,
            interface_hash: None,
            packet_hash: path.packet_hash,
        }],
    }
}

#[test]
#[ignore = "isolated 100k-entry allocator probe; run only this test with --ignored --exact --nocapture"]
fn persistence_codec_allocation_probe() {
    let mode = std::env::var("LXMF_SNAPSHOT_CODEC").unwrap_or_else(|_| "paths".into());
    let paths =
        if mode == "paths" { (0..100_000).map(|i| path_entry(i, 1)).collect() } else { Vec::new() };
    let tunnels = if mode == "tunnels" {
        (0..100_000).map(|i| tunnel_entry(i, 1)).collect()
    } else {
        Vec::new()
    };
    let before = ALLOCATOR.allocated();
    let total_before = ALLOCATOR.total_allocated();
    let start = Instant::now();
    let encoded = if mode == "paths" {
        PathTable::encode_python_entries(&paths)
    } else {
        assert_eq!(mode, "tunnels");
        TunnelTable::encode_python_entries(&tunnels)
    }
    .expect("encode owned metadata");
    let elapsed_ns = start.elapsed().as_nanos();
    let peak_extra = ALLOCATOR.max_allocated().saturating_sub(before);
    let total = ALLOCATOR.total_allocated().saturating_sub(total_before);
    let retained_extra = ALLOCATOR.allocated().saturating_sub(before);
    assert!(
        peak_extra <= encoded.len().saturating_mul(2).saturating_add(1024 * 1024),
        "codec scratch must stay bounded by output size: peak={peak_extra} output={}",
        encoded.len()
    );
    println!(
        "{}",
        serde_json::json!({"mode":mode,"entries":100000,"encoded_bytes":encoded.len(),
        "peak_extra_requested_bytes":peak_extra,"total_requested_bytes":total,
        "retained_extra_requested_bytes":retained_extra,"elapsed_ns":elapsed_ns,
        "sha256":hex::encode(Sha256::digest(&encoded))})
    );
}
