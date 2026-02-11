use serde::Deserialize;

use lxmf::propagation::validate_stamp;

#[derive(Deserialize)]
struct PnStampCase {
    transient_data: Vec<u8>,
    target_cost: u32,
}

#[test]
fn propagation_stamp_validation_matches_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/pn_stamp_valid.msgpack").unwrap();
    let case: PnStampCase = rmp_serde::from_slice(&bytes).unwrap();

    let stamp =
        validate_stamp(&case.transient_data, case.target_cost).expect("expected valid stamp");

    assert_eq!(stamp.transient_id.len(), 32);
    assert_eq!(stamp.stamp.len(), 32);
    assert_eq!(stamp.lxmf_data.len() + stamp.stamp.len(), case.transient_data.len());
}
