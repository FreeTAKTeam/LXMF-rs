use lxmf::message::WireMessage;
use rand_core::{CryptoRng, RngCore};
use reticulum::identity::Identity;

const PROPAGATION_TIMESTAMP: f64 = 1_700_000_000.0;

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
fn propagation_pack_matches_fixture() {
    let packed = std::fs::read("tests/fixtures/python/lxmf/propagation_message.bin")
        .expect("propagation message fixture");
    let fixture = std::fs::read("tests/fixtures/python/lxmf/propagation.bin")
        .expect("propagation packed fixture");
    let dest_pub = std::fs::read("tests/fixtures/python/lxmf/propagation_dest_pubkey.bin")
        .expect("propagation dest pubkey fixture");

    assert_eq!(dest_pub.len(), 64, "destination pubkey fixture length");
    let identity = Identity::new_from_slices(&dest_pub[..32], &dest_pub[32..]);

    let wire = WireMessage::unpack(&packed).expect("valid wire message");

    let packed = wire
        .pack_propagation_with_rng(&identity, PROPAGATION_TIMESTAMP, FixedRng(0x42))
        .expect("propagation pack");

    assert_eq!(packed, fixture);
}
