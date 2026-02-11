use lxmf::error::LxmfError;

#[test]
fn error_variants_format() {
    let err = LxmfError::Decode("payload".into());
    assert!(err.to_string().contains("payload"));
}
