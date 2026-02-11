use serde::Deserialize;

#[derive(Deserialize)]
struct StampCase {
    material: Vec<u8>,
    target_cost: u32,
    stamp: Vec<u8>,
}

#[test]
fn validates_python_stamp() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/stamp_valid.msgpack").unwrap();
    let case: StampCase = rmp_serde::from_slice(&bytes).unwrap();

    let workblock =
        lxmf::stamper::stamp_workblock(&case.material, lxmf::constants::WORKBLOCK_EXPAND_ROUNDS);
    assert!(lxmf::stamper::stamp_valid(&case.stamp, case.target_cost, &workblock));
}
