use serde::Deserialize;
use serde_bytes::ByteBuf;

use lxmf::propagation::{ingest_envelope, validate_stamp};
use lxmf::storage::PropagationStore;

#[derive(Deserialize)]
struct PnStampCase {
    transient_data: Vec<u8>,
    target_cost: u32,
}

#[test]
fn propagation_store_roundtrip() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/pn_stamp_valid.msgpack").unwrap();
    let case: PnStampCase = rmp_serde::from_slice(&bytes).unwrap();
    let stamp = validate_stamp(&case.transient_data, case.target_cost).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let store = PropagationStore::new(tmp.path());

    store
        .save(&stamp.transient_id, &case.transient_data)
        .unwrap();
    let loaded = store.get(&stamp.transient_id).unwrap();
    assert_eq!(loaded, case.transient_data);

    let ids = store.list_ids().unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], stamp.transient_id);
}

#[test]
fn propagation_store_from_envelope() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/pn_stamp_valid.msgpack").unwrap();
    let case: PnStampCase = rmp_serde::from_slice(&bytes).unwrap();

    let envelope = (1.0f64, vec![ByteBuf::from(case.transient_data.clone())]);
    let packed = rmp_serde::to_vec(&envelope).unwrap();

    let ingested = ingest_envelope(&packed, case.target_cost).unwrap();
    assert_eq!(ingested.len(), 1);

    let tmp = tempfile::tempdir().unwrap();
    let store = PropagationStore::new(tmp.path());
    store.save(&ingested[0].transient_id, &ingested[0].lxmf_data).unwrap();

    let loaded = store.get(&ingested[0].transient_id).unwrap();
    assert_eq!(loaded, ingested[0].lxmf_data);
}
