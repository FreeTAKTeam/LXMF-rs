use lxmf::reticulum::Adapter;

#[test]
fn adapter_exports_destination_hash_len() {
    assert_eq!(Adapter::DEST_HASH_LEN, 16);
}
