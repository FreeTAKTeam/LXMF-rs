use super::WireMessage;
use crate::message::{MessageContainer, MessageState, Payload, TransportMethod};
use rand_core::OsRng;
use rns_core::identity::{DecryptIdentity, PrivateIdentity, PUBLIC_KEY_LENGTH};
use serde_bytes::ByteBuf;
use sha2::{Digest, Sha256};
use x25519_dalek::PublicKey;

fn address_hash_bytes(identity: &PrivateIdentity) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(identity.address_hash().as_slice());
    out
}

#[test]
fn propagation_decrypt_preserves_original_wire_and_legacy_salt() {
    use super::{DerivedKey, EphemeralSecret, Fernet, PlainText};
    let sender = PrivateIdentity::new_from_name("fetch-sender");
    let receiver = PrivateIdentity::new_from_name("fetch-recipient");
    let destination = [0x61; 16];
    let mut message = WireMessage::new(destination, address_hash_bytes(&sender),
        Payload::new(1.0, Some(b"original".to_vec()), None, None, None));
    message.sign(&sender).expect("signature");
    let wire = message.pack().expect("wire");
    let (transient, _) = message.pack_propagation_transient_with_rng(receiver.as_identity(), OsRng).expect("transient");
    for stamped in [false, true] {
        let mut transient = transient.clone();
        if stamped { transient.extend_from_slice(&[0x51; 32]); }
        assert_eq!(WireMessage::decrypt_propagation(&transient, &receiver, &destination).expect("decrypt"), wire);
        assert!(WireMessage::decrypt_propagation(&transient, &receiver, &[0x62;16]).is_err());
        assert!(WireMessage::decrypt_propagation(&transient, &sender, &destination).is_err());
    }
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    let shared = secret.diffie_hellman(&receiver.as_identity().public_key);
    let derived = DerivedKey::new(&shared, Some(&destination));
    let key = derived.as_bytes();
    let split = key.len()/2;
    let fernet = Fernet::new_from_slices(&key[..split], &key[split..], OsRng);
    let mut token = vec![0; wire.len()+128];
    let encrypted = fernet.encrypt(PlainText::from(&wire[16..]), &mut token).expect("legacy encryption");
    let mut legacy = destination.to_vec();
    legacy.extend_from_slice(public.as_bytes());
    legacy.extend_from_slice(encrypted.as_bytes());
    assert_eq!(WireMessage::decrypt_propagation(&legacy, &receiver, &destination).expect("legacy decrypt"), wire);
    assert!(WireMessage::decrypt_propagation(&[0; 16], &receiver, &[0;16]).is_err());
}

#[test]
fn propagation_pack_derives_transient_id_from_lxm_data() {
    let sender = PrivateIdentity::new_from_name("propagation-pack-sender");
    let receiver = PrivateIdentity::new_from_name("propagation-pack-receiver");
    let payload = Payload::new(1.0, Some(b"content".to_vec()), Some(b"title".to_vec()), None, None);
    let mut wire =
        WireMessage::new(address_hash_bytes(&receiver), address_hash_bytes(&sender), payload);
    wire.sign(&sender).expect("sign");

    let (envelope, transient_id) = wire
        .pack_propagation_with_options_and_rng(receiver.as_identity(), 2.0, None, OsRng)
        .expect("pack propagation");
    let (_timestamp, entries): (f64, Vec<ByteBuf>) =
        rmp_serde::from_slice(&envelope).expect("decode propagation envelope");
    assert_eq!(entries.len(), 1);

    let expected = Sha256::digest(entries[0].as_ref());
    assert_eq!(transient_id.as_slice(), expected.as_slice());
}

#[test]
fn propagation_pack_appends_optional_stamp_after_lxm_data() {
    let sender = PrivateIdentity::new_from_name("propagation-pack-stamp-sender");
    let receiver = PrivateIdentity::new_from_name("propagation-pack-stamp-receiver");
    let payload = Payload::new(1.0, Some(vec![0x55; 48]), Some(b"title".to_vec()), None, None);
    let mut wire =
        WireMessage::new(address_hash_bytes(&receiver), address_hash_bytes(&sender), payload);
    wire.sign(&sender).expect("sign");

    let propagation_stamp = vec![0xAB; 32];
    let (envelope, transient_id) = wire
        .pack_propagation_with_options_and_rng(
            receiver.as_identity(),
            3.0,
            Some(propagation_stamp.as_slice()),
            OsRng,
        )
        .expect("pack propagation with stamp");
    let (_timestamp, entries): (f64, Vec<ByteBuf>) =
        rmp_serde::from_slice(&envelope).expect("decode propagation envelope");
    let transient_payload = entries[0].as_ref();

    assert!(transient_payload.ends_with(propagation_stamp.as_slice()));
    let lxm_data = &transient_payload[..transient_payload.len() - propagation_stamp.len()];
    let expected = Sha256::digest(lxm_data);
    assert_eq!(transient_id.as_slice(), expected.as_slice());
}

#[test]
fn propagation_transient_helper_matches_envelope_transient_id() {
    let sender = PrivateIdentity::new_from_name("propagation-pack-helper-sender");
    let receiver = PrivateIdentity::new_from_name("propagation-pack-helper-receiver");
    let payload = Payload::new(1.0, Some(vec![0x11; 32]), Some(b"title".to_vec()), None, None);
    let mut wire =
        WireMessage::new(address_hash_bytes(&receiver), address_hash_bytes(&sender), payload);
    wire.sign(&sender).expect("sign");

    let (lxmf_data, transient_id) = wire
        .pack_propagation_transient_with_rng(receiver.as_identity(), OsRng)
        .expect("pack propagation transient");
    let propagation_stamp = vec![0xCD; 32];
    let envelope = WireMessage::pack_propagation_envelope(
        4.0,
        &lxmf_data,
        Some(propagation_stamp.as_slice()),
    )
    .expect("pack propagation envelope");
    let (_timestamp, entries): (f64, Vec<ByteBuf>) =
        rmp_serde::from_slice(&envelope).expect("decode propagation envelope");
    let transient_payload = entries[0].as_ref();

    assert!(transient_payload.ends_with(propagation_stamp.as_slice()));
    assert_eq!(
        &transient_payload[..transient_payload.len() - propagation_stamp.len()],
        lxmf_data.as_slice()
    );
    let expected = Sha256::digest(&lxmf_data);
    assert_eq!(transient_id.as_slice(), expected.as_slice());
}

#[test]
fn propagation_transient_can_be_decrypted_by_recipient_identity() {
    let sender = PrivateIdentity::new_from_name("propagation-pack-decrypt-sender");
    let receiver = PrivateIdentity::new_from_name("propagation-pack-decrypt-receiver");
    let payload = Payload::new(1.0, Some(b"content".to_vec()), Some(b"title".to_vec()), None, None);
    let mut wire =
        WireMessage::new(address_hash_bytes(&receiver), address_hash_bytes(&sender), payload);
    wire.sign(&sender).expect("sign");

    let packed = wire.pack().expect("pack");
    let (lxmf_data, _transient_id) = wire
        .pack_propagation_transient_with_rng(receiver.as_identity(), OsRng)
        .expect("pack propagation transient");
    let encrypted = &lxmf_data[16..];
    let mut ephemeral_pub = [0u8; PUBLIC_KEY_LENGTH];
    ephemeral_pub.copy_from_slice(&encrypted[..PUBLIC_KEY_LENGTH]);
    let derived =
        receiver.derive_key(&PublicKey::from(ephemeral_pub), Some(receiver.address_hash().as_slice()));
    let mut plaintext = vec![0u8; packed.len()];
    let decrypted = receiver
        .decrypt(OsRng, &encrypted[PUBLIC_KEY_LENGTH..], &derived, &mut plaintext)
        .expect("decrypt propagation payload");

    assert_eq!(&lxmf_data[..16], &packed[..16]);
    assert_eq!(decrypted, &packed[16..]);
}

#[test]
fn paper_uri_roundtrip_keeps_delivery_hash_separate_from_identity_hash() {
    let sender = PrivateIdentity::new_from_name("paper-pack-sender");
    let receiver = PrivateIdentity::new_from_name("paper-pack-receiver");
    let mut delivery_hash = [0x42u8; 16];
    assert_ne!(delivery_hash.as_slice(), receiver.address_hash().as_slice());
    let payload = Payload::new(1.0, Some(b"content".to_vec()), Some(b"title".to_vec()), None, None);
    let mut wire = WireMessage::new(delivery_hash, address_hash_bytes(&sender), payload);
    wire.sign(&sender).expect("sign");

    let uri = wire.pack_paper_uri_with_rng(receiver.as_identity(), OsRng).expect("pack paper uri");
    let unpacked = WireMessage::unpack_paper_uri(&uri, &receiver).expect("unpack paper uri");

    assert_eq!(unpacked.destination, delivery_hash);
    assert_eq!(unpacked.source, wire.source);
    assert_eq!(
        unpacked.payload.to_msgpack().expect("payload msgpack"),
        wire.payload.to_msgpack().expect("payload msgpack")
    );
    delivery_hash[0] ^= 0x01;
    assert_ne!(unpacked.destination, delivery_hash);
}

#[test]
fn unpack_storage_accepts_python_msgpack_container() {
    let sender = PrivateIdentity::new_from_name("python-storage-sender");
    let receiver = PrivateIdentity::new_from_name("python-storage-receiver");
    let payload = Payload::new(
        1_773_999_123.25,
        Some(b"content".to_vec()),
        Some(b"title".to_vec()),
        None,
        None,
    );
    let mut wire =
        WireMessage::new(address_hash_bytes(&receiver), address_hash_bytes(&sender), payload);
    wire.sign(&sender).expect("sign");

    let packed_wire = wire.pack().expect("pack");
    let python_container = rmp_serde::to_vec(&rmpv::Value::Map(vec![
        (rmpv::Value::String("state".into()), rmpv::Value::Integer(4_i64.into())),
        (rmpv::Value::String("lxmf_bytes".into()), rmpv::Value::Binary(packed_wire.clone())),
        (rmpv::Value::String("transport_encrypted".into()), rmpv::Value::Boolean(true)),
        (
            rmpv::Value::String("transport_encryption".into()),
            rmpv::Value::String("Curve25519".into()),
        ),
        (rmpv::Value::String("method".into()), rmpv::Value::Integer(2_i64.into())),
    ]))
    .expect("pack python container");

    let unpacked = WireMessage::unpack_storage(&python_container).expect("unpack storage");
    assert_eq!(unpacked.destination, wire.destination);
    assert_eq!(unpacked.source, wire.source);
    assert_eq!(unpacked.signature, wire.signature);
    assert_eq!(
        unpacked.payload.to_msgpack().expect("payload msgpack"),
        wire.payload.to_msgpack().expect("payload msgpack")
    );
}

#[test]
fn pack_storage_emits_python_msgpack_container() {
    let sender = PrivateIdentity::new_from_name("pack-storage-python-sender");
    let receiver = PrivateIdentity::new_from_name("pack-storage-python-receiver");
    let payload = Payload::new(
        1_774_000_123.5,
        Some(b"content".to_vec()),
        Some(b"title".to_vec()),
        None,
        None,
    );
    let mut wire =
        WireMessage::new(address_hash_bytes(&receiver), address_hash_bytes(&sender), payload);
    wire.sign(&sender).expect("sign");

    let storage = wire.pack_storage().expect("pack storage");
    assert!(!storage.starts_with(b"LXMFSTR0"));

    let container = MessageContainer::from_msgpack(&storage).expect("decode container");
    assert_eq!(container.state, MessageState::Outbound.as_u8());
    assert_eq!(container.method, TransportMethod::Direct.as_u8());
    assert!(!container.transport_encrypted);
    assert_eq!(container.transport_encryption, None);
    assert_eq!(container.lxmf_bytes.as_ref(), wire.pack().expect("pack").as_slice());

    let unpacked = WireMessage::unpack_storage(&storage).expect("unpack storage");
    assert_eq!(unpacked.destination, wire.destination);
    assert_eq!(unpacked.source, wire.source);
    assert_eq!(unpacked.signature, wire.signature);
    assert_eq!(
        unpacked.payload.to_msgpack().expect("payload msgpack"),
        wire.payload.to_msgpack().expect("payload msgpack")
    );
}

#[test]
fn pack_storage_container_preserves_python_metadata() {
    let sender = PrivateIdentity::new_from_name("pack-storage-metadata-sender");
    let receiver = PrivateIdentity::new_from_name("pack-storage-metadata-receiver");
    let payload = Payload::new(
        1_774_000_456.75,
        Some(b"content".to_vec()),
        Some(b"title".to_vec()),
        None,
        None,
    );
    let mut wire =
        WireMessage::new(address_hash_bytes(&receiver), address_hash_bytes(&sender), payload);
    wire.sign(&sender).expect("sign");

    let storage = wire
        .pack_storage_container(
            MessageState::Delivered,
            TransportMethod::Propagated,
            true,
            Some("Curve25519".to_string()),
        )
        .expect("pack storage container");
    let container = MessageContainer::from_msgpack(&storage).expect("decode container");

    assert_eq!(container.state, MessageState::Delivered.as_u8());
    assert_eq!(container.method, TransportMethod::Propagated.as_u8());
    assert!(container.transport_encrypted);
    assert_eq!(container.transport_encryption.as_deref(), Some("Curve25519"));
    assert_eq!(container.lxmf_bytes.as_ref(), wire.pack().expect("pack").as_slice());
    assert!(WireMessage::unpack_storage(&storage).is_ok());
}
