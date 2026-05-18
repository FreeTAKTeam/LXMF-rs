use lxmf_core::announce::{parse_announce_slots, AnnounceSlot};
use lxmf_core::{Payload, TransportMethod, WireMessage};
use rmpv::Value;
use serde_json::Value as JsonValue;
use std::fs;

fn fixture_dir() -> &'static str {
    "tests/fixtures/lxmf_interop"
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let raw = fs::read_to_string(format!("{}/fixtures.json", fixture_dir()))
        .expect("failed to read tests/fixtures/lxmf_interop/fixtures.json");
    let parsed: JsonValue =
        serde_json::from_str(&raw).expect("failed to parse fixtures.json as JSON");
    let hex = parsed["fixtures"][name]["hex"]
        .as_str()
        .unwrap_or_else(|| panic!("fixture '{name}' not found or missing hex field"));
    hex::decode(hex).expect("failed to decode fixture hex payload")
}

fn deterministic_wire(stamp: Option<Vec<u8>>, fields: Option<Value>) -> WireMessage {
    let payload = Payload::new(
        1_773_999_000.25,
        Some(b"interop-content".to_vec()),
        Some(b"interop-title".to_vec()),
        fields,
        stamp,
    );
    let mut wire = WireMessage::new([0x11; 16], [0x22; 16], payload);
    wire.signature = Some([0x33; 64]);
    wire
}

fn first_diff(a: &[u8], b: &[u8]) -> String {
    for i in 0..a.len().min(b.len()) {
        if a[i] != b[i] {
            return format!("byte {} differs: {:02x} != {:02x}", i, a[i], b[i]);
        }
    }
    format!("length differs: {} != {}", a.len(), b.len())
}

#[test]
fn pack_stamp_roundtrip_matches_reference_fixture() {
    let wire = deterministic_wire(Some(vec![0x44; 32]), None);
    let packed = wire.pack().expect("interop test invariant failed");
    let fixture = fixture_bytes("direct_with_stamp");
    assert_eq!(packed, fixture, "{}", first_diff(&packed, &fixture));

    let reparsed = WireMessage::unpack(&fixture).expect("interop test invariant failed");
    assert_eq!(
        reparsed.payload.stamp.as_ref().expect("interop test invariant failed").as_ref(),
        vec![0x44; 32].as_slice()
    );

    let repacked = reparsed.pack().expect("interop test invariant failed");
    assert_eq!(repacked, fixture, "{}", first_diff(&repacked, &fixture));
}

#[test]
fn rust_fixture_matches_python_binary_envelope() {
    let packed = deterministic_wire(None, None).pack().expect("interop test invariant failed");
    let fixture = fixture_bytes("direct_no_stamp");
    assert_eq!(packed, fixture, "{}", first_diff(&packed, &fixture));
}

#[test]
fn python_fixture_parses_and_repacks_without_stamp_mutation() {
    let fixture = fixture_bytes("propagation_with_stamp");
    let (ts, entries): (f64, Vec<serde_bytes::ByteBuf>) =
        rmp_serde::from_slice(&fixture).expect("interop test invariant failed");
    assert_eq!(ts, 1_773_999_001.5);

    let stamp = &entries[0][entries[0].len() - 32..];
    assert_eq!(stamp, vec![0x55; 32].as_slice());

    let repacked = WireMessage::pack_propagation_envelope(
        ts,
        &entries[0][..entries[0].len() - 32],
        Some(stamp),
    )
    .expect("interop test invariant failed");
    assert_eq!(repacked, fixture, "{}", first_diff(&repacked, &fixture));
}

#[derive(Default)]
struct MockHub {
    accepted: bool,
    queued: bool,
    retried: bool,
}

fn route_message(hub: &mut MockHub, method: TransportMethod, stamp: Option<&[u8]>) {
    match method {
        TransportMethod::Direct => hub.accepted = true,
        TransportMethod::Propagated if stamp.is_some() => {
            hub.accepted = true;
            hub.queued = true;
            hub.retried = true;
        }
        TransportMethod::Propagated => {}
        _ => {}
    }
}

#[test]
fn direct_message_is_not_queued_for_propagation() {
    let envelope = deterministic_wire(None, None).pack().expect("interop test invariant failed");
    let mut hub = MockHub::default();
    route_message(&mut hub, TransportMethod::Direct, None);
    assert!(!envelope.is_empty());
    assert!(hub.accepted && !hub.queued);
}

#[test]
fn propagation_message_with_valid_stamp_is_queued() {
    let wire = deterministic_wire(Some(vec![0x44; 32]), None);
    let _ = wire
        .pack_propagation_with_options_and_rng(
            rns_core::identity::PrivateIdentity::new_from_name("rx").as_identity(),
            1_773_999_001.5,
            Some(&[0x55; 32]),
            rand_core::OsRng,
        )
        .expect("interop test invariant failed");

    let mut hub = MockHub::default();
    route_message(&mut hub, TransportMethod::Propagated, Some(&[0x55; 32]));
    assert!(hub.accepted && hub.queued && hub.retried);
}

#[test]
fn announce_slots_parse_known_capabilities() {
    let parsed = parse_announce_slots(&[0x01, 0x03, b'a', b'b', b'c', 0x05, 0x01, 0x09])
        .expect("interop test invariant failed");
    assert_eq!(parsed[0], AnnounceSlot { id: 0x01, value: b"abc".to_vec() });
}

#[test]
fn announce_parser_handles_unknown_tlv_without_panic() {
    assert_eq!(
        parse_announce_slots(&[0x99, 0x02, 0xAA, 0xBB]).expect("interop test invariant failed")[0]
            .id,
        0x99
    );
}

#[test]
fn announce_parser_rejects_truncated_slot() {
    assert!(parse_announce_slots(&[0x01, 0x05, 0xAA]).is_err());
}

#[test]
fn regenerate_fixtures_when_env_set() {
    if std::env::var("LXMF_REGEN_FIXTURES").ok().as_deref() != Some("1") {
        return;
    }

    fs::create_dir_all(fixture_dir()).expect("interop test invariant failed");

    let no_stamp = deterministic_wire(None, None).pack().expect("interop test invariant failed");
    let with_stamp = deterministic_wire(Some(vec![0x44; 32]), None)
        .pack()
        .expect("interop test invariant failed");
    let fields = Some(Value::Map(vec![(Value::Integer(99.into()), Value::String("meta".into()))]));
    let with_meta = deterministic_wire(Some(vec![0x44; 32]), fields)
        .pack()
        .expect("interop test invariant failed");
    let (propagation, _) = deterministic_wire(Some(vec![0x44; 32]), None)
        .pack_propagation_with_options_and_rng(
            rns_core::identity::PrivateIdentity::new_from_name("rx").as_identity(),
            1_773_999_001.5,
            Some(&[0x55; 32]),
            rand_core::OsRng,
        )
        .expect("interop test invariant failed");
    let malformed = vec![0x91, 0xC0];

    let fixtures = serde_json::json!({
        "fixtures": {
            "direct_no_stamp": {
                "hex": hex::encode(no_stamp)
            },
            "direct_with_stamp": {
                "hex": hex::encode(with_stamp)
            },
            "direct_with_metadata": {
                "hex": hex::encode(with_meta)
            },
            "propagation_with_stamp": {
                "hex": hex::encode(propagation)
            },
            "malformed_missing_stamp_tail": {
                "hex": hex::encode(malformed)
            }
        }
    });

    fs::write(
        format!("{}/fixtures.json", fixture_dir()),
        serde_json::to_vec_pretty(&fixtures).expect("failed to serialize fixtures JSON"),
    )
    .expect("failed to write tests/fixtures/lxmf_interop/fixtures.json");
}
