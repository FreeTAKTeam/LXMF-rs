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
