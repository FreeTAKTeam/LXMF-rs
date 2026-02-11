use serde::Deserialize;
use serde_bytes::ByteBuf;

use lxmf::propagation::{validate_stamp, PropagationService};
use lxmf::storage::PropagationStore;

#[derive(Deserialize)]
struct PnStampCase {
    transient_data: Vec<u8>,
    target_cost: u32,
}

#[test]
fn propagation_service_ingests_and_fetches() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/pn_stamp_valid.msgpack").unwrap();
    let case: PnStampCase = rmp_serde::from_slice(&bytes).unwrap();

    let envelope = (1.0f64, vec![ByteBuf::from(case.transient_data.clone())]);
    let packed = rmp_serde::to_vec(&envelope).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let store = PropagationStore::new(tmp.path());
    let service = PropagationService::new(store, case.target_cost);

    let count = service.ingest(&packed).unwrap();
    assert_eq!(count, 1);

    let stamped = validate_stamp(&case.transient_data, case.target_cost).unwrap();
    let transient_id = stamped.transient_id;

    let loaded = service.fetch(&transient_id).unwrap();
    assert_eq!(loaded, stamped.lxmf_data);
}
