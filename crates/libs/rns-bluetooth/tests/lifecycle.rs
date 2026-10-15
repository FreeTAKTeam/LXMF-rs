use rns_bluetooth::*;
use std::{sync::Arc, time::Duration};
fn deadline() -> Deadline {
    Deadline::after(Duration::from_secs(2)).unwrap()
}
fn limits() -> Limits {
    Limits { max_sessions: 2, max_events: 2, max_payload_bytes: 4 }
}
#[test]
fn closing_plugin_does_not_close_rnode_or_replacement() {
    let registry = SessionRegistry::new(2).unwrap();
    let rnode = registry.reserve(OwnerId("rnode".into()), DeviceId("one".into())).unwrap();
    let plugin = registry.reserve(OwnerId("plugin".into()), DeviceId("two".into())).unwrap();
    assert_eq!(
        registry.reserve(OwnerId("other".into()), DeviceId("three".into())).unwrap_err().kind,
        ErrorKind::Busy
    );
    assert!(registry.release(&plugin).unwrap());
    let next = registry.reserve(plugin.owner.clone(), DeviceId("two".into())).unwrap();
    assert!(!registry.release(&plugin).unwrap());
    assert!(registry.device(&next).is_ok());
    assert!(registry.device(&rnode).is_ok());
    assert_ne!(next, plugin);
}
#[test]
fn same_device_cannot_be_claimed_by_two_owners() {
    let registry = SessionRegistry::new(2).unwrap();
    registry.reserve(OwnerId("one".into()), DeviceId("radio".into())).unwrap();
    assert_eq!(
        registry.reserve(OwnerId("two".into()), DeviceId("radio".into())).unwrap_err().kind,
        ErrorKind::Busy
    );
}
#[test]
fn close_unblocks_waiter_and_is_not_idle() {
    let q = Arc::new(EventQueue::new(OwnerId("a".into()), limits()).unwrap());
    let worker = q.clone();
    let thread = std::thread::spawn(move || worker.poll(deadline()));
    q.close().unwrap();
    assert_eq!(thread.join().unwrap().unwrap_err().kind, ErrorKind::Closed);
}
#[test]
fn overflow_fails_stream_and_other_owner_queue_survives() {
    let q = EventQueue::new(OwnerId("a".into()), limits()).unwrap();
    let other = EventQueue::new(OwnerId("b".into()), limits()).unwrap();
    let id = SessionId { owner: OwnerId("a".into()), generation: 1 };
    for _ in 0..2 {
        q.push(id.clone(), None, EventKind::Connected).unwrap();
    }
    assert_eq!(
        q.push(id.clone(), None, EventKind::Connected).unwrap_err().kind,
        ErrorKind::QueueFull
    );
    assert_eq!(q.poll(deadline()).unwrap_err().kind, ErrorKind::QueueFull);
    assert_eq!(
        other.push(id, None, EventKind::Connected).unwrap_err().kind,
        ErrorKind::StaleSession
    );
    other
        .push(SessionId { owner: OwnerId("b".into()), generation: 2 }, None, EventKind::Connected)
        .unwrap();
    assert_eq!(other.poll(deadline()).unwrap().unwrap().sequence, 1);
}
#[test]
fn oversized_event_is_not_silently_dropped() {
    let q = EventQueue::new(OwnerId("a".into()), limits()).unwrap();
    let id = SessionId { owner: OwnerId("a".into()), generation: 1 };
    let error = q
        .push(
            id,
            None,
            EventKind::Notification {
                characteristic: CharacteristicId {
                    service: "s".into(),
                    characteristic: "c".into(),
                },
                payload: vec![0; 5],
            },
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::PayloadTooLarge);
}
#[cfg(feature = "android")]
#[test]
fn bridge_version_must_match_before_io() {
    assert!(android::require_bridge_version(android::BRIDGE_VERSION).is_ok());
    assert!(android::require_bridge_version(android::BRIDGE_VERSION + 1).is_err());
}

#[test]
fn cleanup_preserves_overflow_reason() {
    let queue = EventQueue::new(
        OwnerId("owner".into()),
        Limits { max_sessions: 1, max_events: 1, max_payload_bytes: 1 },
    )
    .unwrap();
    let id = SessionId { owner: OwnerId("owner".into()), generation: 1 };
    queue.push(id.clone(), None, EventKind::Connected).unwrap();
    assert_eq!(
        queue.push(id, None, EventKind::Disconnected).unwrap_err().kind,
        ErrorKind::QueueFull
    );
    queue.close().unwrap();
    assert_eq!(
        queue.poll(Deadline::after(std::time::Duration::from_secs(1)).unwrap()).unwrap_err().kind,
        ErrorKind::QueueFull
    );
}
