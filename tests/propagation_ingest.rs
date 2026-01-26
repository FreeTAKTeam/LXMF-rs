use serde::Deserialize;
use serde_bytes::ByteBuf;

use lxmf::propagation::ingest_envelope;

#[derive(Deserialize)]
struct PnStampCase {
    transient_data: Vec<u8>,
    target_cost: u32,
}

#[test]
fn ingest_envelope_accepts_valid_stamp() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/pn_stamp_valid.msgpack").unwrap();
    let case: PnStampCase = rmp_serde::from_slice(&bytes).unwrap();

    let envelope = (1.0f64, vec![ByteBuf::from(case.transient_data.clone())]);
    let packed = rmp_serde::to_vec(&envelope).unwrap();

    let ingested = ingest_envelope(&packed, case.target_cost).unwrap();
    assert_eq!(ingested.len(), 1);
    assert_eq!(ingested[0].transient_id.len(), 32);
    assert!(ingested[0].stamp_value.is_some());
}
