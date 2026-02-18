use lxmf::handlers::{
    DeliveryAnnounceHandler, PropagationAnnounceEvent, PropagationAnnounceHandler,
};
use std::sync::{Arc, Mutex};

#[test]
fn delivery_handler_runs_callback() {
    let called: Arc<Mutex<Vec<[u8; 16]>>> = Arc::new(Mutex::new(Vec::new()));
    let called_cb = Arc::clone(&called);

    let mut handler = DeliveryAnnounceHandler::with_callback(Box::new(move |dest| {
        called_cb.lock().expect("callback state").push(*dest);
        Ok(())
    }));

    let dest = [0xAB; 16];
    handler.handle(&dest).unwrap();

    let seen = called.lock().expect("callback state");
    assert_eq!(seen.as_slice(), &[dest]);
}

#[test]
fn propagation_handler_parses_app_data_and_runs_callback() {
    let fixture = std::fs::read("tests/fixtures/python/lxmf/propagation_node_app_data_custom.bin")
        .expect("custom propagation fixture");

    let seen: Arc<Mutex<Option<PropagationAnnounceEvent>>> = Arc::new(Mutex::new(None));
    let seen_cb = Arc::clone(&seen);
    let mut handler = PropagationAnnounceHandler::with_callback(Box::new(move |event| {
        *seen_cb.lock().expect("callback state") = Some(event.clone());
        Ok(())
    }));

    let dest = [0xCD; 16];
    let event = handler.handle_with_app_data(&dest, &fixture).unwrap();
    assert_eq!(event.destination, dest);
    assert_eq!(event.name.as_deref(), Some("TestNode"));
    assert_eq!(event.stamp_cost, Some(20));
    assert_eq!(event.stamp_cost_flexibility, Some(4));
    assert_eq!(event.peering_cost, Some(25));

    let seen_event = seen.lock().expect("callback state").clone().unwrap();
    assert_eq!(seen_event, event);
}

#[test]
fn propagation_handler_accepts_invalid_msgpack_gracefully() {
    let mut handler = PropagationAnnounceHandler::new();
    let result = handler.handle_with_app_data(&[0x01; 16], b"invalid-msgpack");
    assert!(result.is_ok());
    let event = result.expect("invalid app-data should still be handled");
    assert_eq!(event.name, None);
    assert_eq!(event.stamp_cost, None);
    assert_eq!(event.stamp_cost_flexibility, None);
    assert_eq!(event.peering_cost, None);
}

#[test]
fn propagation_handler_accepts_parseable_but_non_standard_app_data() {
    let payload = rmp_serde::to_vec(&(false, 1_700_000_000u64, true, 111u32, 222u32, vec![20u32]))
        .expect("non-standard announce payload");

    let seen: Arc<Mutex<Option<PropagationAnnounceEvent>>> = Arc::new(Mutex::new(None));
    let seen_cb = Arc::clone(&seen);
    let mut handler = PropagationAnnounceHandler::with_callback(Box::new(move |event| {
        *seen_cb.lock().expect("callback state") = Some(event.clone());
        Ok(())
    }));

    let dest = [0x22; 16];
    let event =
        handler.handle_with_app_data(&dest, &payload).expect("non-standard announce accepted");

    assert_eq!(event.destination, dest);
    assert_eq!(event.name, None);
    assert_eq!(event.stamp_cost, Some(20));
    assert_eq!(event.stamp_cost_flexibility, None);
    assert_eq!(event.peering_cost, None);

    let seen_event = seen.lock().expect("callback state").clone().unwrap();
    assert_eq!(seen_event, event);
}
