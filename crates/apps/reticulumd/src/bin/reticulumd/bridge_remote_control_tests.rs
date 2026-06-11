use super::*;
use reticulum_daemon::lxmf_stamps::generate_propagation_stamp;
use sha2::{Digest, Sha256};

#[test]
fn propagation_remote_fetch_summary_reports_transferred_bytes() {
    let payloads = vec![b"first".to_vec(), b"second-payload".to_vec()];

    let summary = propagation_remote_fetch_summary(7, &payloads, 1, 2, 3);

    assert_eq!(summary["available_count"].as_u64(), Some(7));
    assert_eq!(summary["fetched_count"].as_u64(), Some(2));
    assert_eq!(summary["imported_count"].as_u64(), Some(1));
    assert_eq!(summary["duplicate_count"].as_u64(), Some(2));
    assert_eq!(summary["rejected_count"].as_u64(), Some(3));
    assert_eq!(
        summary["transferred_bytes"].as_u64(),
        Some(payloads.iter().map(Vec::len).sum::<usize>() as u64)
    );
}

#[test]
fn propagation_control_response_code_maps_peer_errors_like_python() {
    for (code, kind, message) in [
        (0xF3_u64, std::io::ErrorKind::PermissionDenied, "propagation peer invalid peering key"),
        (0xF5_u64, std::io::ErrorKind::PermissionDenied, "propagation peer invalid stamp"),
        (0xF6_u64, std::io::ErrorKind::WouldBlock, "propagation peer throttled"),
        (0xFE_u64, std::io::ErrorKind::TimedOut, "propagation peer timed out"),
    ] {
        let err = response_code_error(&rmpv::Value::from(code))
            .expect("peer response code should map to error");

        assert_eq!(err.kind(), kind);
        assert_eq!(err.to_string(), message);
    }
}

#[test]
fn propagation_remote_fetch_ack_payload_reports_imported_and_duplicate_haves() {
    let imported_payload = b"imported remote fetch payload".to_vec();
    let duplicate_lxm_data = vec![0x51; 160];
    let duplicate_transient_id = Sha256::digest(&duplicate_lxm_data);
    let duplicate_stamp = generate_propagation_stamp(
        duplicate_transient_id.as_slice().try_into().expect("transient id width"),
        1,
    )
    .expect("propagation stamp");
    let mut duplicate_payload = duplicate_lxm_data.clone();
    duplicate_payload.extend_from_slice(duplicate_stamp.as_slice());
    let rejected_payload = b"rejected remote fetch payload".to_vec();

    let ack = propagation_remote_fetch_ack_payload(&[
        (&imported_payload, LocalPropagationImportOutcome::Imported),
        (&duplicate_payload, LocalPropagationImportOutcome::Duplicate),
        (&rejected_payload, LocalPropagationImportOutcome::Rejected),
    ]);

    let rmpv::Value::Array(entries) = ack else {
        panic!("expected /get acknowledgement array");
    };
    assert!(entries.first().is_some_and(rmpv::Value::is_nil));
    let Some(rmpv::Value::Array(haves)) = entries.get(1) else {
        panic!("expected haves array");
    };
    assert_eq!(haves.len(), 2);
    assert_eq!(haves[0], rmpv::Value::Binary(Sha256::digest(imported_payload).to_vec()));
    assert_eq!(haves[1], rmpv::Value::Binary(duplicate_transient_id.to_vec()));
    assert_ne!(haves[1], rmpv::Value::Binary(Sha256::digest(duplicate_payload).to_vec()));
}
