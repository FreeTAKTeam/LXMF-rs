use crate::transport::DeliveryReceipt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Recovery policy for a poisoned receipt-mapping lock (issue #513):
/// recover the map and keep operating.
///
/// The map holds only plain `String -> String` receipt mappings with no
/// cross-entry invariants, so a panic while holding the lock can at worst
/// leave one stale or missing entry — both of which the existing
/// `NotFound` and prune paths already tolerate. Discarding the lock (and
/// silently dropping every subsequent track/prune/lookup for the rest of
/// the process lifetime, the old behavior) is strictly worse than
/// recovering it.
///
/// Once the map state has been accepted, the poison flag is cleared
/// while still holding the recovered guard, so the recovery applies
/// process-wide: later acquisitions — including call sites that lock the
/// map directly — no longer see a poison error, and the warning is
/// emitted once per poisoning instead of on every operation.
fn lock_receipt_map(
    map: &Arc<Mutex<HashMap<String, String>>>,
) -> MutexGuard<'_, HashMap<String, String>> {
    map.lock().unwrap_or_else(|poisoned| {
        log::warn!("receipt mapping lock poisoned; recovering map state and continuing");
        let guard = poisoned.into_inner();
        map.clear_poison();
        guard
    })
}

pub fn resolve_receipt_message_id(
    map: &Arc<Mutex<HashMap<String, String>>>,
    receipt: &DeliveryReceipt,
) -> std::io::Result<String> {
    let key = hex::encode(receipt.message_id);
    let mut guard = lock_receipt_map(map);
    guard.remove(&key).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "receipt mapping not found")
    })
}

pub fn lookup_receipt_message_id(
    map: &Arc<Mutex<HashMap<String, String>>>,
    receipt: &DeliveryReceipt,
) -> std::io::Result<String> {
    let key = hex::encode(receipt.message_id);
    let guard = lock_receipt_map(map);
    guard.get(&key).cloned().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "receipt mapping not found")
    })
}

pub fn track_receipt_mapping(
    map: &Arc<Mutex<HashMap<String, String>>>,
    packet_hash: &str,
    message_id: &str,
) {
    let mut guard = lock_receipt_map(map);
    guard.insert(packet_hash.to_string(), message_id.to_string());
}

pub fn prune_receipt_mappings_for_message(
    map: &Arc<Mutex<HashMap<String, String>>>,
    message_id: &str,
) {
    let mut guard = lock_receipt_map(map);
    guard.retain(|_, mapped_message_id| mapped_message_id != message_id);
}

pub trait ReceiptRecordSink {
    fn record_receipt_status(&self, message_id: &str, status: &str) -> std::io::Result<()>;
}

impl<F> ReceiptRecordSink for F
where
    F: Fn(&str, &str) -> std::io::Result<()>,
{
    fn record_receipt_status(&self, message_id: &str, status: &str) -> std::io::Result<()> {
        self(message_id, status)
    }
}

pub fn record_receipt_status(
    sink: &impl ReceiptRecordSink,
    message_id: &str,
    status: &str,
) -> Result<(), std::io::Error> {
    sink.record_receipt_status(message_id, status)
}

#[cfg(test)]
mod tests {
    use super::{
        lookup_receipt_message_id, prune_receipt_mappings_for_message, resolve_receipt_message_id,
        track_receipt_mapping,
    };
    use crate::transport::DeliveryReceipt;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    type ReceiptMap = Arc<Mutex<HashMap<String, String>>>;

    fn receipt(byte: u8) -> DeliveryReceipt {
        DeliveryReceipt::new([byte; 32])
    }

    /// Poisons the map's mutex the way a panic mid-operation would: a
    /// thread takes the lock and panics while holding it.
    fn poison_map(map: &ReceiptMap) {
        let poisoned = map.clone();
        let handle = std::thread::spawn(move || {
            let _guard = poisoned.lock().expect("lock before poisoning");
            panic!("simulated panic while holding the receipt mapping lock");
        });
        assert!(handle.join().is_err(), "poisoning thread must panic");
        assert!(map.lock().is_err(), "map must be poisoned after the panic");
    }

    #[test]
    fn receipt_mapping_roundtrip_resolves_and_prunes() {
        let map: ReceiptMap = Arc::new(Mutex::new(HashMap::new()));
        let key = hex::encode(receipt(0x11).message_id);

        track_receipt_mapping(&map, &key, "msg-1");
        assert_eq!(lookup_receipt_message_id(&map, &receipt(0x11)).expect("lookup"), "msg-1");
        // Lookup does not consume; resolve does.
        assert_eq!(lookup_receipt_message_id(&map, &receipt(0x11)).expect("lookup"), "msg-1");
        assert_eq!(resolve_receipt_message_id(&map, &receipt(0x11)).expect("resolve"), "msg-1");
        assert!(resolve_receipt_message_id(&map, &receipt(0x11)).is_err());

        track_receipt_mapping(&map, &key, "msg-2");
        track_receipt_mapping(&map, "other-hash", "msg-3");
        prune_receipt_mappings_for_message(&map, "msg-2");
        assert!(lookup_receipt_message_id(&map, &receipt(0x11)).is_err());
        assert_eq!(map.lock().expect("lock").get("other-hash"), Some(&"msg-3".to_string()));
    }

    // Regression tests for issue #513: after a panic poisons the mapping
    // lock, every receipt operation must recover the map and keep working
    // instead of logging-and-dropping for the rest of the process.
    #[test]
    fn track_receipt_mapping_recovers_after_lock_poisoning() {
        let map: ReceiptMap = Arc::new(Mutex::new(HashMap::new()));
        poison_map(&map);

        let key = hex::encode(receipt(0x01).message_id);
        track_receipt_mapping(&map, &key, "msg-1");
        assert_eq!(lookup_receipt_message_id(&map, &receipt(0x01)).expect("lookup"), "msg-1");
        // Recovery applies process-wide: the poison flag was cleared
        // while accepting the recovered map, so even call sites that lock
        // the map directly (e.g. the daemon's failed-send cleanup) no
        // longer see a poison error.
        assert!(
            map.lock().is_ok(),
            "poison flag must be cleared once the recovered map is accepted"
        );
    }

    #[test]
    fn lookup_and_resolve_receipt_message_id_recover_after_lock_poisoning() {
        let map: ReceiptMap = Arc::new(Mutex::new(HashMap::new()));
        let key = hex::encode(receipt(0x22).message_id);
        track_receipt_mapping(&map, &key, "msg-1");
        poison_map(&map);

        // Previously both returned a poison error; now they recover the
        // map and find the pre-poison entry still intact.
        assert_eq!(lookup_receipt_message_id(&map, &receipt(0x22)).expect("lookup"), "msg-1");
        assert_eq!(resolve_receipt_message_id(&map, &receipt(0x22)).expect("resolve"), "msg-1");
        assert!(resolve_receipt_message_id(&map, &receipt(0x22)).is_err());
    }

    #[test]
    fn prune_receipt_mappings_recovers_after_lock_poisoning() {
        let map: ReceiptMap = Arc::new(Mutex::new(HashMap::new()));
        track_receipt_mapping(&map, &hex::encode(receipt(0x31).message_id), "msg-1");
        track_receipt_mapping(&map, &hex::encode(receipt(0x32).message_id), "msg-2");
        poison_map(&map);

        prune_receipt_mappings_for_message(&map, "msg-1");
        assert!(lookup_receipt_message_id(&map, &receipt(0x31)).is_err());
        assert_eq!(lookup_receipt_message_id(&map, &receipt(0x32)).expect("lookup"), "msg-2");
    }
}
