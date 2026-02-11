use serde::Deserialize;
use serde_bytes::ByteBuf;

use lxmf::propagation::validate_stamp;
use lxmf::router::Router;

#[derive(Deserialize)]
struct PnStampCase {
    transient_data: Vec<u8>,
    target_cost: u32,
}

#[test]
fn router_ingests_and_fetches_propagation() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/pn_stamp_valid.msgpack").unwrap();
    let case: PnStampCase = rmp_serde::from_slice(&bytes).unwrap();
    let stamped = validate_stamp(&case.transient_data, case.target_cost).unwrap();

    let envelope = (1.0f64, vec![ByteBuf::from(case.transient_data.clone())]);
    let packed = rmp_serde::to_vec(&envelope).unwrap();

    let temp = tempfile::tempdir().unwrap();
    let mut router = Router::default();
    router.enable_propagation(temp.path(), case.target_cost);
    assert!(router.propagation_enabled());

    let count = router.ingest_propagation(&packed).unwrap();
    assert_eq!(count, 1);
    assert_eq!(router.propagation_ingested_total(), 1);
    assert_eq!(router.last_ingest_count(), 1);

    let loaded = router.fetch_propagated(&stamped.transient_id).unwrap();
    assert_eq!(loaded, stamped.lxmf_data);
}
