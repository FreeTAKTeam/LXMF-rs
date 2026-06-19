use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(super) const OUTBOUND_RESOURCE_SENT_STATUS: &str = "sent: link resource";

pub(super) type OutboundResourceMap = Arc<Mutex<HashMap<String, OutboundResourceTracking>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OutboundResourceTracking {
    pub(super) message_id: String,
    pub(super) peer: String,
    pub(super) bytes: usize,
    pub(super) sent_status: String,
}

pub(super) fn track_outbound_resource(
    outbound_resource_map: &OutboundResourceMap,
    resource_hash_hex: String,
    tracking: OutboundResourceTracking,
) {
    if let Ok(mut guard) = outbound_resource_map.lock() {
        guard.insert(resource_hash_hex, tracking);
    }
}

pub(super) fn take_outbound_resource_tracking(
    outbound_resource_map: &OutboundResourceMap,
    resource_hash_hex: &str,
) -> Option<OutboundResourceTracking> {
    match outbound_resource_map.lock() {
        Ok(mut guard) => guard.remove(resource_hash_hex),
        Err(err) => {
            log::warn!("[daemon] failed to lock outbound resource map: {err}");
            None
        }
    }
}

pub(super) fn prune_outbound_resource_mappings_for_message(
    outbound_resource_map: &OutboundResourceMap,
    message_id: &str,
) {
    if let Ok(mut guard) = outbound_resource_map.lock() {
        guard.retain(|_, tracking| tracking.message_id != message_id);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        prune_outbound_resource_mappings_for_message, take_outbound_resource_tracking,
        track_outbound_resource, OutboundResourceTracking, OUTBOUND_RESOURCE_SENT_STATUS,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn outbound_resource_tracking_round_trips_and_prunes() {
        let map = Arc::new(Mutex::new(HashMap::new()));
        track_outbound_resource(
            &map,
            "res-1".to_string(),
            OutboundResourceTracking {
                message_id: "msg-1".to_string(),
                peer: "peer-a".to_string(),
                bytes: 128,
                sent_status: OUTBOUND_RESOURCE_SENT_STATUS.to_string(),
            },
        );
        track_outbound_resource(
            &map,
            "res-2".to_string(),
            OutboundResourceTracking {
                message_id: "msg-2".to_string(),
                peer: "peer-b".to_string(),
                bytes: 256,
                sent_status: OUTBOUND_RESOURCE_SENT_STATUS.to_string(),
            },
        );

        let tracking = take_outbound_resource_tracking(&map, "res-1").expect("resource mapping");
        assert_eq!(tracking.message_id, "msg-1");
        assert_eq!(tracking.peer, "peer-a");
        assert_eq!(tracking.bytes, 128);
        assert_eq!(tracking.sent_status, OUTBOUND_RESOURCE_SENT_STATUS);

        prune_outbound_resource_mappings_for_message(&map, "msg-2");
        assert!(take_outbound_resource_tracking(&map, "res-2").is_none());
    }

    #[test]
    fn outbound_resource_completion_stays_non_terminal() {
        assert_eq!(OUTBOUND_RESOURCE_SENT_STATUS, "sent: link resource");
    }
}
