use serde::Deserialize;

#[derive(Deserialize)]
struct PnStampCase {
    transient_data: Vec<u8>,
    target_cost: u32,
}

#[test]
fn pn_stamp_validation_matches_python_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/pn_stamp_valid.msgpack").unwrap();
    let case: PnStampCase = rmp_serde::from_slice(&bytes).unwrap();

    let result = lxmf::stamper::validate_pn_stamp(&case.transient_data, case.target_cost);
    assert!(result.is_some());
}
