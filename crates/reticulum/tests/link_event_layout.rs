use reticulum::destination::link::{LinkEvent, LinkPayload};
use reticulum::rpc::RpcDaemon;

#[test]
fn link_event_handles_boxed_payload() {
    let payload = LinkPayload::default();
    let event = LinkEvent::Data(Box::new(payload));
    match event {
        LinkEvent::Data(_) => {}
        _ => panic!("expected data event"),
    }
}

#[test]
fn link_events_emit_on_activate() {
    let daemon = RpcDaemon::test_instance();
    daemon.emit_link_event_for_test();
    let event = daemon.take_event().expect("link event");
    assert_eq!(event.event_type, "link_activated");
}
