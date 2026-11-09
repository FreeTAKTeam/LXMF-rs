//! Run the ownership probe alone: the allocator high-water is process-global.
use super::snapshot_allocation_tests::ALLOCATOR;
use super::*;

#[test]
#[ignore = "isolated requested-live reference-set lifetime probe; one fresh process per mode"]
fn reference_set_lifetime_allocation_probe() {
    const ROWS: usize = 100_000;
    let mode = std::env::var("LXMF_REFERENCE_SET_MODE").expect("retained or scoped");
    let hashes =
        (0..ROWS).map(|index| Hash::new_from_slice(&index.to_be_bytes())).collect::<Vec<_>>();
    let mut state = AnnounceCacheState::default();
    state.files.reserve(ROWS + 1);
    // Some snapshot keys are first inserted by the successful packet loop.
    for hash in &hashes[..ROWS - 10] {
        state.files.insert(*hash, [0; 32]);
    }
    let orphan = Hash::new_from_slice(b"old-unreferenced-cache-key");
    state.files.insert(orphan, [0xff; 32]);
    let packet = Arc::new(Packet::default());
    let mut packets = Vec::with_capacity(ROWS + 3);
    for hash in &hashes {
        packets.push((*hash, AddressHash::new_empty(), Arc::clone(&packet)));
    }
    // Preserve repeated active/tunnel hashes with distinct interface variants.
    for byte in 1..=3 {
        packets.push((
            hashes[1],
            AddressHash::new_from_hash(&Hash::new([byte; 32])),
            Arc::clone(&packet),
        ));
    }
    let before = ALLOCATOR.allocated();
    let total_before = ALLOCATOR.total_allocated();
    let fixture_prior_peak = ALLOCATOR.max_allocated().saturating_sub(before);
    let retained = if mode == "retained" {
        let mut current = HashSet::with_capacity(packets.len());
        current.extend(packets.iter().map(|(hash, _, _)| *hash));
        state.files.retain(|hash, _| current.contains(hash));
        Some(current)
    } else {
        assert_eq!(mode, "scoped");
        state.retain_current_files(packets.iter().map(|(hash, _, _)| *hash));
        None
    };
    let after_retain_live = ALLOCATOR.allocated().saturating_sub(before);
    for (hash, iface, _) in &packets {
        let mut fingerprint = [0; 32];
        fingerprint[..crate::hash::ADDRESS_HASH_SIZE].copy_from_slice(iface.as_slice());
        state.files.insert(*hash, fingerprint);
    }
    // Both metadata writes and cleanup occur after this point in production.
    // Keep the old set observably live through the old publication boundary.
    std::hint::black_box(&retained);
    let publication_live = ALLOCATOR.allocated().saturating_sub(before);
    let construction_peak = ALLOCATOR.max_allocated().saturating_sub(before);
    let total_requested = ALLOCATOR.total_allocated().saturating_sub(total_before);
    let set_capacity = retained.as_ref().map(HashSet::capacity);
    assert_eq!(state.files.len(), ROWS);
    assert!(hashes.iter().all(|hash| state.files.contains_key(hash)));
    assert!(!state.files.contains_key(&orphan));
    assert_eq!(
        &state.files[&hashes[1]][..crate::hash::ADDRESS_HASH_SIZE],
        &[3; crate::hash::ADDRESS_HASH_SIZE]
    );
    if let Some(current) = &retained {
        assert_eq!(current.len(), state.files.len());
        assert!(current.iter().all(|hash| state.files.contains_key(hash)));
        assert!(publication_live > 0);
    } else {
        assert_eq!(publication_live, 0, "temporary set must be gone before I/O");
    }
    drop(retained);
    let after_release_live = ALLOCATOR.allocated().saturating_sub(before);
    assert_eq!(after_release_live, 0);
    println!(
        "{}",
        serde_json::json!({"mode":mode,"current_hashes":ROWS,"packet_refs":packets.len(),
        "set_capacity_observed":set_capacity,"after_retain_requested_live_bytes":after_retain_live,
        "publication_requested_live_bytes":publication_live,"after_release_requested_live_bytes":after_release_live,
        "construction_global_peak_extra_requested_bytes":construction_peak,"fixture_prior_peak_extra_requested_bytes":fixture_prior_peak,
        "total_requested_bytes":total_requested,"final_map_keys_equal_snapshot_union":true,
        "duplicate_interface_last_fingerprint_preserved":true,
        "scope":"production retaining helper versus old ownership span; in-memory lifetime model only, no filesystem/RSS/cgroup or construction-peak reduction claim"})
    );
}
