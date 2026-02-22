use lxmf_sdk::{
    validate_mobile_ble_capabilities, validate_mobile_ble_event_payload_bounds,
    validate_mobile_ble_event_sequence, MobileBleCapabilities, MobileBleEvent,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

#[test]
fn android_fixture_events_follow_mobile_ble_contract() {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/fixtures/mobile-ble/android/events.sample.json");
    let fixture = fs::read_to_string(&fixture_path).expect("read android fixture");
    let payload: Value = serde_json::from_str(&fixture).expect("parse android fixture json");

    let events: Vec<MobileBleEvent> =
        serde_json::from_value(payload["events"].clone()).expect("deserialize android events");
    let capabilities: MobileBleCapabilities =
        serde_json::from_value(payload["capabilities"].clone()).expect("deserialize capabilities");
    validate_mobile_ble_capabilities(&capabilities).expect("capabilities should satisfy contract");
    validate_mobile_ble_event_sequence(&events).expect("android events should satisfy contract");
    validate_mobile_ble_event_payload_bounds(&events, &capabilities)
        .expect("android events should satisfy payload/timeout bounds");
}
