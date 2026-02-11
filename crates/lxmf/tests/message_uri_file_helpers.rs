use lxmf::message::{Payload, WireMessage};
use rand_core::{CryptoRng, RngCore};
use reticulum::identity::{Identity, PrivateIdentity};

#[derive(Clone, Copy)]
struct FixedRng(u8);

impl RngCore for FixedRng {
    fn next_u32(&mut self) -> u32 {
        u32::from_le_bytes([self.0; 4])
    }

    fn next_u64(&mut self) -> u64 {
        u64::from_le_bytes([self.0; 8])
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        dest.fill(self.0);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for FixedRng {}

#[test]
fn paper_uri_helpers_roundtrip_bytes() {
    let fixture =
        std::fs::read("tests/fixtures/python/lxmf/paper.bin").expect("paper packed fixture");

    let uri = WireMessage::encode_lxm_uri(&fixture);
    assert!(uri.starts_with("lxm://"));

    let decoded = WireMessage::decode_lxm_uri(&uri).expect("uri decode");
    assert_eq!(decoded, fixture);
}

#[test]
fn paper_uri_pack_matches_paper_pack_with_fixed_rng() {
    let packed = std::fs::read("tests/fixtures/python/lxmf/paper_message.bin")
        .expect("paper message fixture");
    let dest_pub = std::fs::read("tests/fixtures/python/lxmf/propagation_dest_pubkey.bin")
        .expect("dest pubkey fixture");
    assert_eq!(dest_pub.len(), 64, "destination pubkey fixture length");
    let identity = Identity::new_from_slices(&dest_pub[..32], &dest_pub[32..]);

    let wire = WireMessage::unpack(&packed).expect("valid wire message");
    let expected = wire.pack_paper_with_rng(&identity, FixedRng(0x42)).expect("paper pack");
    let uri = wire.pack_paper_uri_with_rng(&identity, FixedRng(0x42)).expect("paper uri");
    let decoded = WireMessage::decode_lxm_uri(&uri).expect("uri decode");

    assert_eq!(decoded, expected);
}

#[test]
fn paper_uri_decode_rejects_invalid_inputs() {
    assert!(WireMessage::decode_lxm_uri("http://not-lxm").is_err());
    assert!(WireMessage::decode_lxm_uri("lxm://!@#$").is_err());
}

#[test]
fn wire_file_helpers_roundtrip_wire_and_storage_formats() {
    let signer = PrivateIdentity::new_from_name("wire-file-helper");
    let mut wire = WireMessage::new(
        [0x31; 16],
        [0x32; 16],
        Payload::new(
            1_700_000_000.0,
            Some(b"file-content".to_vec()),
            Some(b"file-title".to_vec()),
            Some(rmpv::Value::Map(vec![(
                rmpv::Value::String("k".into()),
                rmpv::Value::String("v".into()),
            )])),
            None,
        ),
    );
    wire.sign(&signer).expect("sign");

    let temp = tempfile::tempdir().expect("tempdir");
    let wire_path = temp.path().join("wire.lxm");
    let storage_path = temp.path().join("wire.storage");

    wire.pack_to_file(&wire_path).expect("write wire");
    wire.pack_storage_to_file(&storage_path).expect("write storage wire");

    let decoded_wire = WireMessage::unpack_from_file(&wire_path).expect("read wire");
    let decoded_storage =
        WireMessage::unpack_storage_from_file(&storage_path).expect("read storage wire");

    assert_eq!(decoded_wire.destination, wire.destination);
    assert_eq!(decoded_wire.source, wire.source);
    assert_eq!(decoded_wire.signature, wire.signature);
    assert_eq!(decoded_wire.payload, wire.payload);

    assert_eq!(decoded_storage.destination, wire.destination);
    assert_eq!(decoded_storage.source, wire.source);
    assert_eq!(decoded_storage.signature, wire.signature);
    assert_eq!(decoded_storage.payload, wire.payload);
}
