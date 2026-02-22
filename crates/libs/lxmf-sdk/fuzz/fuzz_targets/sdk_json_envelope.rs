#![no_main]

use libfuzzer_sys::fuzz_target;
use lxmf_sdk::{
    ConfigPatch, DeliverySnapshot, EventBatch, RuntimeSnapshot, SdkError, SdkEvent,
};

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<EventBatch>(data);
    let _ = serde_json::from_slice::<SdkEvent>(data);
    let _ = serde_json::from_slice::<SdkError>(data);
    let _ = serde_json::from_slice::<RuntimeSnapshot>(data);
    let _ = serde_json::from_slice::<DeliverySnapshot>(data);
    let _ = serde_json::from_slice::<ConfigPatch>(data);
});
