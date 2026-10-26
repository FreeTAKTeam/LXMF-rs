use super::*;

#[test]
fn removed_routes_are_pruned_from_the_persistence_cache() {
    let retained = Hash::new_from_slice(b"retained-route");
    let removed = Hash::new_from_slice(b"removed-route");
    let mut state = AnnounceCacheState::default();
    state.files.insert(retained, [1; 32]);
    state.files.insert(removed, [2; 32]);

    state.retain_current_files(&HashSet::from([retained]));

    assert_eq!(state.files.len(), 1);
    assert!(state.files.contains_key(&retained));
    assert!(!state.files.contains_key(&removed));
}

#[test]
fn full_verification_is_rate_limited_to_five_minutes() {
    let now = Instant::now();
    let mut state = AnnounceCacheState::default();
    assert!(state.full_verification_due(now));

    state.last_full_verify = Some(now - ANNOUNCE_CACHE_VERIFY_INTERVAL + Duration::from_secs(1));
    assert!(!state.full_verification_due(now));

    state.last_full_verify = Some(now - ANNOUNCE_CACHE_VERIFY_INTERVAL);
    assert!(state.full_verification_due(now));
}
