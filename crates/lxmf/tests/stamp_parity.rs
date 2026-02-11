use serde::Deserialize;

#[derive(Deserialize)]
struct StampCase {
    material: Vec<u8>,
    target_cost: u32,
    stamp: Vec<u8>,
    expected_value: u32,
}

#[test]
fn stamp_verifies_against_python_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/stamp_valid.msgpack").unwrap();
    let case: StampCase = rmp_serde::from_slice(&bytes).unwrap();

    let workblock =
        lxmf::stamper::stamp_workblock(&case.material, lxmf::constants::WORKBLOCK_EXPAND_ROUNDS);
    assert!(lxmf::stamper::stamp_valid(&case.stamp, case.target_cost, &workblock));
    assert_eq!(lxmf::stamper::stamp_value(&workblock, &case.stamp), case.expected_value);
}

#[test]
fn stamp_rejects_invalid_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/stamp_invalid.msgpack").unwrap();
    let case: StampCase = rmp_serde::from_slice(&bytes).unwrap();

    let workblock =
        lxmf::stamper::stamp_workblock(&case.material, lxmf::constants::WORKBLOCK_EXPAND_ROUNDS);
    assert!(!lxmf::stamper::stamp_valid(&case.stamp, case.target_cost, &workblock));
}
