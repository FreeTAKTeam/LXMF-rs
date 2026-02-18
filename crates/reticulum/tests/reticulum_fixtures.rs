#[test]
fn destination_hash_fixture_exists() {
    assert!(std::path::Path::new("tests/fixtures/python/reticulum/destination_hash.bin").exists());
}
