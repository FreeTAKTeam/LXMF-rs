use lxmf::message::WireMessage;
use lxmf::router::Router;

#[test]
fn ingest_lxm_uri_decodes_and_tracks_paper_message() {
    let paper = std::fs::read("tests/fixtures/python/lxmf/paper.bin").expect("paper fixture");
    let uri = WireMessage::encode_lxm_uri(&paper);
    let mut router = Router::default();

    let ingest = router.ingest_lxm_uri(&uri).expect("ingest");
    let mut expected_destination = [0u8; 16];
    expected_destination.copy_from_slice(&paper[..16]);

    assert_eq!(ingest.destination, expected_destination);
    assert_eq!(ingest.bytes_len, paper.len());
    assert!(!ingest.duplicate);
    assert_eq!(router.paper_message_count(), 1);
    assert_eq!(
        router
            .paper_message(&ingest.transient_id)
            .expect("stored paper"),
        paper.as_slice()
    );
    assert_eq!(router.stats().paper_uri_ingested_total, 1);
    assert_eq!(router.stats().paper_uri_duplicate_total, 0);

    let peer = router.peer(&expected_destination).expect("peer");
    assert_eq!(peer.queued_items(), 1);
    assert!(router.process_peer_queues(&expected_destination));
    let peer = router.peer(&expected_destination).expect("peer");
    assert_eq!(peer.unhandled_message_count(), 1);
}

#[test]
fn ingest_lxm_uri_detects_duplicates() {
    let paper = std::fs::read("tests/fixtures/python/lxmf/paper.bin").expect("paper fixture");
    let uri = WireMessage::encode_lxm_uri(&paper);
    let mut router = Router::default();

    let first = router.ingest_lxm_uri(&uri).expect("first ingest");
    let second = router.ingest_lxm_uri(&uri).expect("second ingest");

    assert!(!first.duplicate);
    assert!(second.duplicate);
    assert_eq!(router.paper_message_count(), 1);
    assert_eq!(router.stats().paper_uri_ingested_total, 1);
    assert_eq!(router.stats().paper_uri_duplicate_total, 1);
}

#[test]
fn ingest_lxm_uri_rejects_invalid_data() {
    let mut router = Router::default();

    assert!(router.ingest_lxm_uri("http://invalid").is_err());

    let tiny_uri = WireMessage::encode_lxm_uri(&[0x01; 8]);
    assert!(router.ingest_lxm_uri(&tiny_uri).is_err());
}
