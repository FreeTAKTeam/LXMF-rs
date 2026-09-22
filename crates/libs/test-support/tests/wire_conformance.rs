use lxmf_core::WireMessage;
use rns_core::packet::{
    ContextFlag, DestinationType, HeaderType, Packet, PacketContext, PacketType,
};
use serde::Deserialize;
use std::collections::HashMap;

const FIXTURE: &str =
    include_str!("../../../../tools/interop/python-rust-wire-conformance-v1.json");

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u8,
    generator: Generator,
    vectors: Vec<Vector>,
    negative_vectors: Vec<NegativeVector>,
}

#[derive(Debug, Deserialize)]
struct Generator {
    reticulum_revision: String,
    lxmf_revision: String,
}

#[derive(Debug, Deserialize)]
struct Vector {
    id: String,
    direction: String,
    wire_hex: String,
    expected: Option<Expected>,
}

#[derive(Debug, Deserialize)]
struct Expected {
    destination_hex: Option<String>,
    source_hex: Option<String>,
    title_utf8: Option<String>,
    content_utf8: Option<String>,
    timestamp: Option<f64>,
    header_type: Option<String>,
    packet_type: Option<String>,
    destination_type: Option<String>,
    context: Option<String>,
    payload_utf8: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NegativeVector {
    id: String,
    kind: String,
    wire_hex: Option<String>,
    base_vector: Option<String>,
    truncate_bytes: Option<usize>,
    replace_last_byte_hex: Option<String>,
}

fn manifest() -> Manifest {
    serde_json::from_str(FIXTURE).expect("wire conformance fixture must be valid JSON")
}

fn bytes(hex: &str) -> Vec<u8> {
    hex::decode(hex).expect("wire conformance fixture must contain valid hex")
}

fn vector<'a>(vectors: &'a [Vector], id: &str) -> &'a Vector {
    vectors.iter().find(|vector| vector.id == id).expect("required wire vector is present")
}

#[test]
fn manifest_provenance_and_directions_are_explicit() {
    let fixture = manifest();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.generator.reticulum_revision, "99de23c040d507e3fefca19e87b182302902725d");
    assert_eq!(fixture.generator.lxmf_revision, "727830cefda83d9c6e3982b48675425f3f988f9c");

    let directions = fixture
        .vectors
        .iter()
        .map(|vector| (vector.id.as_str(), vector.direction.as_str()))
        .collect::<HashMap<_, _>>();
    assert_eq!(directions.get("python_plain_packet"), Some(&"python_to_rust"));
    assert_eq!(directions.get("python_lxmf_wire"), Some(&"python_to_rust"));
    assert_eq!(directions.get("rust_lxmf_wire"), Some(&"rust_to_python"));
    assert!(fixture.negative_vectors.iter().any(|vector| vector.id == "packet_empty_payload"));
    assert!(fixture
        .negative_vectors
        .iter()
        .any(|vector| vector.id == "lxmf_reserved_msgpack_code"));
}

#[test]
fn python_encoded_bytes_are_decoded_and_repacked_by_rust() {
    let fixture = manifest();

    let packet_vector = vector(&fixture.vectors, "python_plain_packet");
    let expected = packet_vector.expected.as_ref().expect("packet expectations");
    let packet = Packet::from_bytes(&bytes(&packet_vector.wire_hex)).expect("Python packet wire");
    assert_eq!(expected.header_type.as_deref(), Some("type1"));
    assert_eq!(expected.packet_type.as_deref(), Some("data"));
    assert_eq!(expected.destination_type.as_deref(), Some("plain"));
    assert_eq!(expected.context.as_deref(), Some("none"));
    assert_eq!(packet.header.header_type, HeaderType::Type1);
    assert_eq!(packet.header.packet_type, PacketType::Data);
    assert_eq!(packet.header.destination_type, DestinationType::Plain);
    assert_eq!(packet.header.context_flag, ContextFlag::Unset);
    assert_eq!(packet.context, PacketContext::None);
    assert_eq!(packet.data.as_slice(), expected.payload_utf8.as_deref().unwrap().as_bytes());
    assert_eq!(packet.to_bytes().expect("packet repack"), bytes(&packet_vector.wire_hex));

    let lxmf_vector = vector(&fixture.vectors, "python_lxmf_wire");
    let expected = lxmf_vector.expected.as_ref().expect("LXMF expectations");
    let wire_bytes = bytes(&lxmf_vector.wire_hex);
    let message = WireMessage::unpack(&wire_bytes).expect("Python LXMF wire");
    assert_eq!(hex::encode(message.destination), expected.destination_hex.as_deref().unwrap());
    assert_eq!(hex::encode(message.source), expected.source_hex.as_deref().unwrap());
    assert_eq!(
        message.payload.title.as_ref().map(|value| value.as_slice()),
        Some(expected.title_utf8.as_deref().unwrap().as_bytes())
    );
    assert_eq!(
        message.payload.content.as_ref().map(|value| value.as_slice()),
        Some(expected.content_utf8.as_deref().unwrap().as_bytes())
    );
    assert_eq!(message.payload.timestamp, expected.timestamp.unwrap());
    assert_eq!(message.pack().expect("LXMF repack"), wire_bytes);
}

#[test]
fn rust_encoded_bytes_remain_a_python_consumable_fixture() {
    let fixture = manifest();
    let vector = vector(&fixture.vectors, "rust_lxmf_wire");
    let expected = vector.expected.as_ref().expect("LXMF expectations");
    let wire_bytes = bytes(&vector.wire_hex);
    let message = WireMessage::unpack(&wire_bytes).expect("Rust LXMF fixture");
    assert_eq!(hex::encode(message.destination), expected.destination_hex.as_deref().unwrap());
    assert_eq!(hex::encode(message.source), expected.source_hex.as_deref().unwrap());
    assert_eq!(
        message.payload.title.as_ref().map(|value| value.as_slice()),
        Some(expected.title_utf8.as_deref().unwrap().as_bytes())
    );
    assert_eq!(
        message.payload.content.as_ref().map(|value| value.as_slice()),
        Some(expected.content_utf8.as_deref().unwrap().as_bytes())
    );
    assert_eq!(message.payload.timestamp, expected.timestamp.unwrap());
    assert_eq!(message.pack().expect("Rust fixture repack"), wire_bytes);
}

#[test]
fn deliberately_corrupted_wire_vectors_are_rejected() {
    let fixture = manifest();
    let vectors = fixture
        .vectors
        .iter()
        .map(|vector| (vector.id.as_str(), vector))
        .collect::<HashMap<_, _>>();

    for negative in &fixture.negative_vectors {
        let wire = if let Some(hex) = &negative.wire_hex {
            bytes(hex)
        } else {
            let base_id = negative.base_vector.as_deref().expect("negative base vector");
            let base = bytes(&vectors.get(base_id).expect("negative base exists").wire_hex);
            if let Some(count) = negative.truncate_bytes {
                assert!(count < base.len(), "{} truncation is in bounds", negative.id);
                base[..base.len() - count].to_vec()
            } else if let Some(last) = &negative.replace_last_byte_hex {
                let mut corrupted = base;
                let replacement = bytes(last);
                assert_eq!(replacement.len(), 1, "{} replacement is one byte", negative.id);
                *corrupted.last_mut().expect("base has a last byte") = replacement[0];
                corrupted
            } else {
                panic!("{} has no corruption operation", negative.id);
            }
        };

        match negative.kind.as_str() {
            "reticulum_packet" => {
                assert!(Packet::from_bytes(&wire).is_err(), "{} must be rejected", negative.id);
            }
            "lxmf_wire" => {
                assert!(WireMessage::unpack(&wire).is_err(), "{} must be rejected", negative.id);
            }
            other => panic!("{} has unsupported negative kind {other}", negative.id),
        }
    }
}
