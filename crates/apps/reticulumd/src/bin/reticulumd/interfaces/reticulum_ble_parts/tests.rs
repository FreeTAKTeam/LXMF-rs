use super::{
    fragment_packet, FragmentError, PeerRegistration, ReticulumBleRole, ReticulumBleRuntimeCore,
};

use std::time::{Duration, Instant};

fn ident(byte: u8) -> [u8; 16] {
    [byte; 16]
}

#[test]
fn reticulum_ble_fragment_codec_matches_pinned_golden_vectors() {
    let fragments = fragment_packet(b"Hello, Reticulum!", 185).expect("fragment");
    assert_eq!(fragments.len(), 1);
    assert_eq!(&fragments[0][..5], &[0x01, 0x00, 0x00, 0x00, 0x01]);
    assert_eq!(&fragments[0][5..], b"Hello, Reticulum!");

    let fragments = fragment_packet(&vec![b'A'; 500], 185).expect("fragment");
    assert_eq!(fragments.len(), 3);
    assert_eq!(&fragments[0][..5], &[0x01, 0x00, 0x00, 0x00, 0x03]);
    assert_eq!(&fragments[1][..5], &[0x02, 0x00, 0x01, 0x00, 0x03]);
    assert_eq!(&fragments[2][..5], &[0x03, 0x00, 0x02, 0x00, 0x03]);
    assert_eq!(fragments[0].len(), 185);
    assert_eq!(fragments[1].len(), 185);
    assert_eq!(fragments[2].len(), 145);
}

#[test]
fn reticulum_ble_reassembles_lone_and_out_of_order_fragments() {
    let now = Instant::now();
    let mut core = ReticulumBleRuntimeCore::new(ident(0x10));
    let peer = ident(0x20);
    let lone = fragment_packet(b"short", 185).expect("fragment");
    assert_eq!(
        core.receive_fragment(peer, &lone[0], now).expect("receive"),
        Some(b"short".to_vec())
    );

    let original = vec![b'F'; 150];
    let fragments = fragment_packet(&original, 50).expect("fragment");
    assert_eq!(fragments.len(), 4);
    assert_eq!(core.receive_fragment(peer, &fragments[0], now).expect("receive"), None);
    assert_eq!(core.receive_fragment(peer, &fragments[2], now).expect("receive"), None);
    assert_eq!(core.receive_fragment(peer, &fragments[1], now).expect("receive"), None);
    assert_eq!(core.receive_fragment(peer, &fragments[3], now).expect("receive"), Some(original));
}

#[test]
fn reticulum_ble_rejects_duplicate_mismatch_timeout_and_oversize() {
    let now = Instant::now();
    let mut core = ReticulumBleRuntimeCore::new(ident(0x10));
    let peer = ident(0x20);
    let fragments = fragment_packet(&[b'X'; 200], 100).expect("fragment");
    assert_eq!(core.receive_fragment(peer, &fragments[0], now).expect("receive"), None);
    assert_eq!(core.receive_fragment(peer, &fragments[1], now).expect("receive"), None);
    let mut changed = fragments[1].clone();
    changed[5] = b'Y';
    assert!(matches!(
        core.receive_fragment(peer, &changed, now),
        Err(FragmentError::DuplicateMismatch(1))
    ));

    let stale = fragment_packet(&[b'Z'; 200], 100).expect("fragment");
    assert_eq!(core.receive_fragment(peer, &stale[0], now).expect("receive"), None);
    let later = now + Duration::from_secs(31);
    assert_eq!(core.receive_fragment(peer, &stale[1], later).expect("receive"), None);
    assert_eq!(core.counters.stale_reassembly_drops, 1);

    let oversize = vec![0_u8; 513];
    assert!(matches!(
        core.receive_fragment(peer, &oversize, later),
        Err(FragmentError::Oversize(513))
    ));
}

#[test]
fn reticulum_ble_tracks_identity_mac_rotation_and_role_deduplication() {
    let mut core = ReticulumBleRuntimeCore::new(ident(0x10));
    let peer = ident(0x20);
    assert_eq!(
        core.register_peer(peer, "AA:BB:CC:00:00:01", ReticulumBleRole::Central, 185),
        PeerRegistration::Added
    );
    assert_eq!(
        core.register_peer(peer, "AA:BB:CC:00:00:02", ReticulumBleRole::Central, 247),
        PeerRegistration::Updated
    );
    assert_eq!(core.active_address(&peer), Some("AA:BB:CC:00:00:02"));
    assert_eq!(
        core.register_peer(peer, "AA:BB:CC:00:00:03", ReticulumBleRole::Peripheral, 247),
        PeerRegistration::DuplicateRejected { retained_role: ReticulumBleRole::Central }
    );
    assert_eq!(core.active_address(&peer), Some("AA:BB:CC:00:00:02"));
    assert_eq!(core.counters.duplicate_rejections, 1);
}

#[test]
fn reticulum_ble_role_policy_keeps_peripheral_when_peer_identity_is_lower() {
    let mut core = ReticulumBleRuntimeCore::new(ident(0x30));
    let peer = ident(0x20);
    assert_eq!(
        core.register_peer(peer, "central-attempt", ReticulumBleRole::Central, 185),
        PeerRegistration::Added
    );
    assert_eq!(
        core.register_peer(peer, "peripheral-attempt", ReticulumBleRole::Peripheral, 185),
        PeerRegistration::DuplicateRejected { retained_role: ReticulumBleRole::Peripheral }
    );
    assert_eq!(core.active_address(&peer), Some("peripheral-attempt"));
}
