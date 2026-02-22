const ALGORITHM_SET_PRIORITY: &[&str] = &["rns-a3", "rns-a2", "rns-a1"];

fn negotiate_algorithm_set(client_supported: &[&str], server_supported: &[&str]) -> Option<String> {
    for algorithm_set in ALGORITHM_SET_PRIORITY {
        if client_supported.iter().any(|candidate| candidate == algorithm_set)
            && server_supported.iter().any(|candidate| candidate == algorithm_set)
        {
            return Some((*algorithm_set).to_string());
        }
    }
    None
}

#[test]
fn sdk_conformance_crypto_agility_negotiates_highest_shared_algorithm_set() {
    let selected = negotiate_algorithm_set(&["rns-a3", "rns-a2", "rns-a1"], &["rns-a2", "rns-a1"]);
    assert_eq!(selected.as_deref(), Some("rns-a2"));
}

#[test]
fn sdk_conformance_crypto_agility_fails_when_no_algorithm_overlap_exists() {
    let selected = negotiate_algorithm_set(&["rns-a3", "rns-a2"], &["rns-a1"]);
    assert!(selected.is_none());
}

#[test]
fn sdk_conformance_crypto_agility_prevents_unlisted_downgrade_selection() {
    let selected = negotiate_algorithm_set(&["rns-a2"], &["rns-a2", "rns-a1"]);
    assert_eq!(selected.as_deref(), Some("rns-a2"));
}
