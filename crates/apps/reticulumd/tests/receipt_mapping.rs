use reticulum_daemon::receipt_bridge::track_receipt_mapping;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[test]
fn track_receipt_mapping_inserts_message_id() {
    let map = Arc::new(Mutex::new(HashMap::new()));
    track_receipt_mapping(&map, "hash-1", "msg-1");
    assert_eq!(map.lock().unwrap().get("hash-1").unwrap(), "msg-1");
}
