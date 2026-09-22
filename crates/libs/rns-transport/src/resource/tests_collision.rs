#[test]
fn resource_sender_regenerates_random_hash_after_a_map_collision() {
    let parts = vec![b"first".to_vec(), b"second".to_vec(), b"third".to_vec()];
    let candidates = [[0u8; RANDOM_HASH_SIZE], [1u8; RANDOM_HASH_SIZE]];
    let mut candidate_index = 0;

    let (random_hash, map_hashes, _, _) = select_collision_free_random_hash(
        b"logical-resource",
        &parts,
        || {
            let candidate = candidates[candidate_index];
            candidate_index += 1;
            candidate
        },
        |part, random_hash| {
            if random_hash[0] == 0 && part == b"second" {
                [b'f', 0, 0, 0]
            } else {
                [part[0], random_hash[0], 0, 0]
            }
        },
    )
    .expect("hash derivation should succeed");

    assert_eq!(random_hash, candidates[1], "the colliding candidate must be discarded");
    assert_eq!(candidate_index, 2, "regeneration should happen exactly once");
    assert_eq!(map_hashes, vec![[b'f', 1, 0, 0], [b's', 1, 0, 0], [b't', 1, 0, 0]]);
}

#[test]
fn resource_sender_allows_map_hashes_repeated_outside_the_collision_guard() {
    let mut parts = Vec::with_capacity(COLLISION_GUARD_SIZE + 1);
    parts.push(b"first".to_vec());
    for index in 1..=COLLISION_GUARD_SIZE {
        parts.push(vec![index as u8]);
    }
    parts.push(b"first".to_vec());

    let (random_hash, map_hashes, _, _) = select_collision_free_random_hash(
        b"logical-resource",
        &parts,
        || [7u8; RANDOM_HASH_SIZE],
        |part, _| {
            if part == b"first" {
                [0xAA; MAPHASH_LEN]
            } else {
                [part[0], 0, 0, 0]
            }
        },
    )
    .expect("the first hash has left the rolling guard by the final fragment");

    assert_eq!(random_hash, [7u8; RANDOM_HASH_SIZE]);
    assert_eq!(map_hashes.len(), parts.len());
}
