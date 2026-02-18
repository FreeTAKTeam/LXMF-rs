#[test]
fn matches_python_address_hash() {
    let input = b"test";
    let expected = std::fs::read("tests/fixtures/python/reticulum/hash_address.bin").unwrap();
    let got = reticulum::hash::address_hash(input);
    assert_eq!(got.as_slice(), expected.as_slice());
}
