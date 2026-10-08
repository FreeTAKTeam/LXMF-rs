//! Stream valid signed/encrypted historical payloads for a local resource soak.
//! Deterministic test identities are public fixture keys, never deployment keys.
use lxmf_core::message::{Payload, WireMessage};
use rand_core::OsRng;
use rns_core::destination::DestinationName;
use rns_core::identity::PrivateIdentity;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::{self, BufWriter, Write};

fn delivery_hash(identity: &PrivateIdentity) -> [u8; 16] {
    let name = DestinationName::new("lxmf", "delivery");
    let hash = Sha256::new()
        .chain_update(name.as_name_hash_slice())
        .chain_update(identity.address_hash().as_slice())
        .finalize();
    let mut result = [0; 16];
    result.copy_from_slice(&hash[..16]);
    result
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: propagation_resource_fixture <count> <content-bytes>".into());
    }
    let count = args[0].parse::<usize>()?;
    let content_bytes = args[1].parse::<usize>()?;
    if count == 0 || !(32..=65_536).contains(&content_bytes) {
        return Err("count must be positive; content bytes must be 32..=65536".into());
    }
    let sender = PrivateIdentity::new_from_name("rch-resource-fixture-sender-public-test-key");
    let receiver = PrivateIdentity::new_from_name("rch-resource-fixture-receiver-public-test-key");
    let destination = delivery_hash(&receiver);
    let source = delivery_hash(&sender);
    let mut output = BufWriter::new(io::stdout().lock());
    for sequence in 0..count {
        let prefix = format!("resource-history-{sequence:08}:");
        let mut content = prefix.into_bytes();
        content.resize(content_bytes, b'x');
        let payload = Payload::new(
            1_790_000_000.0 + sequence as f64,
            Some(content),
            Some(b"resource fixture".to_vec()),
            None,
            None,
        );
        let mut wire = WireMessage::new(destination, source, payload);
        wire.sign(&sender)?;
        let (transient, id) =
            wire.pack_propagation_transient_with_rng(receiver.as_identity(), OsRng)?;
        // Round-trip every row through authenticated decryption and signature validation.
        let decoded = WireMessage::unpack_paper(&transient, &receiver)?;
        if !decoded.verify(sender.as_identity())? || decoded.payload != wire.payload {
            return Err(format!("fixture validation failed at sequence {sequence}").into());
        }
        serde_json::to_writer(
            &mut output,
            &json!({"transient_id":hex::encode(id), "destination":hex::encode(destination),
                "payload_hex":hex::encode(&transient), "size_bytes":transient.len(),
                "signature_and_decryption_verified":true, "stamp_cost":0}),
        )?;
        output.write_all(b"\n")?;
    }
    output.flush()?;
    Ok(())
}
