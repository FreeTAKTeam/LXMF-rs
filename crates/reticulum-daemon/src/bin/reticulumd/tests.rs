use crate::bridge_helpers::opportunistic_payload;
use reticulum::delivery::send_outcome_status;
use reticulum::destination_hash::parse_destination_hash_required;
use reticulum::transport::SendPacketOutcome;

#[test]
fn opportunistic_payload_strips_destination_prefix() {
    let destination = [0xAA; 16];
    let mut payload = destination.to_vec();
    payload.extend_from_slice(&[1, 2, 3, 4]);
    assert_eq!(opportunistic_payload(&payload, &destination), &[1, 2, 3, 4]);
}

#[test]
fn opportunistic_payload_keeps_payload_without_prefix() {
    let destination = [0xAA; 16];
    let payload = vec![0xBB; 24];
    assert_eq!(opportunistic_payload(&payload, &destination), payload.as_slice());
}

#[test]
fn send_outcome_status_maps_success() {
    assert_eq!(
        send_outcome_status("opportunistic", SendPacketOutcome::SentDirect),
        "sent: opportunistic"
    );
}

#[test]
fn send_outcome_status_maps_failures() {
    assert_eq!(
        send_outcome_status("opportunistic", SendPacketOutcome::DroppedMissingDestinationIdentity),
        "failed: opportunistic missing destination identity"
    );
    assert_eq!(
        send_outcome_status("opportunistic", SendPacketOutcome::DroppedNoRoute),
        "failed: opportunistic no route"
    );
}

#[test]
fn parse_destination_hex_required_rejects_invalid_hashes() {
    let err = parse_destination_hash_required("not-hex").expect_err("invalid hash");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}
