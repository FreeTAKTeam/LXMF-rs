use crate::ratchets::now_secs;
use core::num::Wrapping;
use rand_core::OsRng;
use rand_core::{CryptoRng, RngCore};
use tempfile::TempDir;

use crate::buffer::OutputBuffer;
use crate::error::RnsError;
use crate::hash::Hash;
use crate::identity::PrivateIdentity;
use crate::serde::Serialize;

use super::DestinationAnnounce;
use super::DestinationName;
use super::SingleInputDestination;
use super::RATCHET_LENGTH;

#[derive(Clone, Copy)]
struct FixedRng {
    next: Wrapping<u8>,
}

impl FixedRng {
    fn new(seed: u8) -> Self {
        Self { next: Wrapping(seed) }
    }
}

impl RngCore for FixedRng {
    fn next_u32(&mut self) -> u32 {
        let mut bytes = [0u8; 4];
        self.fill_bytes(&mut bytes);
        u32::from_le_bytes(bytes)
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        self.fill_bytes(&mut bytes);
        u64::from_le_bytes(bytes)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for slot in dest.iter_mut() {
            *slot = self.next.0;
            self.next += Wrapping(1);
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for FixedRng {}

fn decode_announce_random_blob(announce: &crate::packet::Packet) -> [u8; 10] {
    let payload = announce.data.as_slice();
    let start = 32 + 32 + 10;
    let end = start + 10;
    let mut blob = [0u8; 10];
    blob.copy_from_slice(&payload[start..end]);
    blob
}

#[test]
fn create_announce() {
    let identity = PrivateIdentity::new_from_rand(OsRng);

    let mut single_in_destination =
        SingleInputDestination::new(identity, DestinationName::new("test", "in"));

    let announce_packet =
        single_in_destination.announce(OsRng, None).expect("valid announce packet");

    println!("Announce packet {}", announce_packet);
}

#[test]
fn create_path_request_hash() {
    let name = DestinationName::new("rnstransport", "path.request");

    println!("PathRequest Name Hash {}", name.hash);
    println!("PathRequest Destination Hash {}", Hash::new_from_slice(name.as_name_hash_slice()));
}

#[test]
fn compare_announce() {
    let priv_key: [u8; 32] = [
        0xf0, 0xec, 0xbb, 0xa4, 0x9e, 0x78, 0x3d, 0xee, 0x14, 0xff, 0xc6, 0xc9, 0xf1, 0xe1, 0x25,
        0x1e, 0xfa, 0x7d, 0x76, 0x29, 0xe0, 0xfa, 0x32, 0x41, 0x3c, 0x5c, 0x59, 0xec, 0x2e, 0x0f,
        0x6d, 0x6c,
    ];

    let sign_priv_key: [u8; 32] = [
        0xf0, 0xec, 0xbb, 0xa4, 0x9e, 0x78, 0x3d, 0xee, 0x14, 0xff, 0xc6, 0xc9, 0xf1, 0xe1, 0x25,
        0x1e, 0xfa, 0x7d, 0x76, 0x29, 0xe0, 0xfa, 0x32, 0x41, 0x3c, 0x5c, 0x59, 0xec, 0x2e, 0x0f,
        0x6d, 0x6c,
    ];

    let priv_identity = PrivateIdentity::new(priv_key.into(), sign_priv_key.into());

    println!("identity hash {}", priv_identity.as_identity().address_hash);

    let mut destination = SingleInputDestination::new(
        priv_identity,
        DestinationName::new("example_utilities", "announcesample.fruits"),
    );

    println!("destination name hash {}", destination.desc.name.hash);
    println!("destination hash {}", destination.desc.address_hash);

    let announce = destination.announce(OsRng, None).expect("valid announce packet");

    let mut output_data = [0u8; 4096];
    let mut buffer = OutputBuffer::new(&mut output_data);

    let _ = announce.serialize(&mut buffer).expect("correct data");

    println!("ANNOUNCE {}", buffer);
}

#[test]
fn check_announce() {
    let priv_identity = PrivateIdentity::new_from_rand(OsRng);

    let mut destination = SingleInputDestination::new(
        priv_identity,
        DestinationName::new("example_utilities", "announcesample.fruits"),
    );

    let announce = destination.announce(OsRng, None).expect("valid announce packet");

    DestinationAnnounce::validate(&announce).expect("valid announce");
}

#[test]
fn announce_signature_covers_app_data() {
    let priv_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut destination = SingleInputDestination::new(
        priv_identity,
        DestinationName::new("example_utilities", "announcesample.fruits"),
    );

    let app_data = b"Rust announce app-data";
    let announce = destination.announce(OsRng, Some(app_data)).expect("valid announce packet");

    let mut tampered = announce;
    let payload = tampered.data.as_mut_slice();
    let app_data_offset = 32 + 32 + 10 + 10 + 64;
    assert!(payload.len() > app_data_offset, "announce must include app_data");
    payload[app_data_offset] ^= 0x01;

    match DestinationAnnounce::validate(&tampered) {
        Ok(_) => panic!("tampered app_data should fail signature verification"),
        Err(err) => assert!(matches!(err, RnsError::IncorrectSignature)),
    }
}

#[test]
fn announce_includes_ratchet_when_enabled() {
    let temp = TempDir::new().expect("temp dir");
    let priv_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut destination = SingleInputDestination::new(
        priv_identity,
        DestinationName::new("example_utilities", "announcesample.fruits"),
    );
    let ratchet_path = temp
        .path()
        .join("ratchets")
        .join(format!("{}.ratchets", destination.desc.address_hash.to_hex_string()));
    destination.enable_ratchets(&ratchet_path).expect("enable ratchets");

    let announce = destination.announce(OsRng, None).expect("valid announce packet");
    let info = DestinationAnnounce::validate(&announce).expect("valid announce");
    assert!(info.ratchet.is_some());
}

#[test]
fn announce_without_ratchet_flag_ignores_ratchet_bytes() {
    let priv_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut destination = SingleInputDestination::new(
        priv_identity,
        DestinationName::new("example_utilities", "announcesample.fruits"),
    );

    let app_data = vec![0u8; RATCHET_LENGTH];
    let announce = destination.announce(OsRng, Some(&app_data)).expect("valid announce packet");
    let info = DestinationAnnounce::validate(&announce).expect("valid announce");
    assert!(info.ratchet.is_none());
    assert_eq!(info.app_data, app_data.as_slice());
}

#[test]
fn announce_random_blob_matches_python_layout() {
    let priv_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut destination = SingleInputDestination::new(
        priv_identity,
        DestinationName::new("example_utilities", "announcesample.fruits"),
    );
    let before = now_secs().floor() as u64;
    let announce = destination.announce(FixedRng::new(0x11), None).expect("valid announce");
    let after = now_secs().floor() as u64;

    let blob = decode_announce_random_blob(&announce);
    assert_eq!(&blob[..5], &[0x11, 0x12, 0x13, 0x14, 0x15]);

    let mut ts_bytes = [0u8; 8];
    ts_bytes[3..8].copy_from_slice(&blob[5..10]);
    let emitted = u64::from_be_bytes(ts_bytes);
    assert!(emitted >= before.saturating_sub(1));
    assert!(emitted <= after.saturating_add(1));
}
