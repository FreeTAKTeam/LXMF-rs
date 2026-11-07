use lxmf_core::constants::{STRUCT_OVERHEAD, TIMESTAMP_SIZE};
use lxmf_core::{decide_delivery, TransportMethod, WireMessage};

#[test]
fn serialized_delivery_boundaries_match_lxmf_1_2_0() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../test-support/fixtures/lxmf_1_2_0_sizes.json"))
            .expect("fixture");
    for case in fixture["cases"].as_array().expect("cases") {
        let bytes = hex::decode(case["wire_hex"].as_str().expect("wire hex")).expect("hex");
        let wire = WireMessage::unpack(&bytes).expect("Python message");
        assert_eq!(
            wire.payload.stamp.as_ref().map_or(0, |stamp| stamp.len()) as u64,
            case["stamp_bytes"],
            "{}",
            case["name"]
        );
        assert_eq!(wire.pack().expect("Rust serialization"), bytes, "{}", case["name"]);
        assert_eq!(bytes.len() as u64, case["wire_bytes"].as_u64().expect("wire size"));
        let content_size = wire
            .payload
            .to_msgpack()
            .expect("payload")
            .len()
            .saturating_sub(TIMESTAMP_SIZE + STRUCT_OVERHEAD);
        let decision = decide_delivery(TransportMethod::Opportunistic, false, content_size)
            .expect("delivery choice");
        assert_eq!(decision.method.as_u8() as u64, case["method"], "{}", case["name"]);
        assert_eq!(decision.representation as u64, case["representation"], "{}", case["name"]);
    }
}
