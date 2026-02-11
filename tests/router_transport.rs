use lxmf::error::LxmfError;
use lxmf::message::{Payload, WireMessage};
use lxmf::reticulum::Adapter;
use lxmf::router::{OutboundStatus, Router};
use std::sync::{Arc, Mutex};

fn make_message(destination: [u8; 16], source: [u8; 16]) -> WireMessage {
    let payload = Payload::new(1_700_000_000.0, Some(b"test".to_vec()), None, None, None);
    WireMessage::new(destination, source, payload)
}

#[test]
fn router_accepts_reticulum_adapter() {
    let adapter = Adapter::new();
    let _router = Router::with_adapter(adapter);
}

#[test]
fn router_uses_adapter_sender_for_outbound_messages() {
    let delivered: Arc<Mutex<Vec<[u8; 16]>>> = Arc::new(Mutex::new(Vec::new()));
    let delivered_cb = Arc::clone(&delivered);
    let adapter = Adapter::with_outbound_sender(move |message| {
        delivered_cb
            .lock()
            .expect("delivered state")
            .push(message.destination);
        Ok(())
    });

    let mut router = Router::with_adapter(adapter);
    router.set_auth_required(true);
    let destination = [0xA1; 16];
    router.allow_destination(destination);
    router.enqueue_outbound(make_message(destination, [0xB2; 16]));

    let result = router.handle_outbound(1).expect("outbound processing");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].status, OutboundStatus::Sent);
    assert_eq!(
        delivered.lock().expect("delivered state").as_slice(),
        &[destination]
    );
}

#[test]
fn router_requeues_when_adapter_send_fails() {
    let adapter = Adapter::with_outbound_sender(|_message| {
        Err(LxmfError::Io("simulated adapter failure".into()))
    });
    let mut router = Router::with_adapter(adapter);
    router.set_auth_required(true);
    let destination = [0xA5; 16];
    router.allow_destination(destination);

    let message = make_message(destination, [0xB5; 16]);
    let message_id = message.message_id().to_vec();
    router.enqueue_outbound(message);

    let result = router.handle_outbound(1).expect("outbound processing");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].status, OutboundStatus::DeferredAdapterError);
    assert_eq!(router.stats().outbound_adapter_errors_total, 1);
    assert_eq!(router.outbound_progress(&message_id), Some(0));
    assert_eq!(router.outbound_len(), 1);
}

#[test]
fn router_does_not_retry_deferred_no_adapter_in_same_batch() {
    let mut router = Router::default();
    let message = make_message([0xA9; 16], [0xB9; 16]);
    let message_id = message.message_id().to_vec();
    router.enqueue_outbound(message);

    let result = router.handle_outbound(5).expect("outbound processing");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].status, OutboundStatus::DeferredNoAdapter);
    assert_eq!(result[0].message_id, message_id);
    assert_eq!(router.outbound_len(), 1);
}

#[test]
fn router_with_unconfigured_adapter_defers_without_dropping() {
    let mut router = Router::with_adapter(Adapter::new());
    router.set_auth_required(true);
    let destination = [0xEA; 16];
    router.allow_destination(destination);
    let message = make_message(destination, [0xFB; 16]);
    let message_id = message.message_id().to_vec();
    router.enqueue_outbound(message);

    let result = router.handle_outbound(5).expect("outbound processing");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].status, OutboundStatus::DeferredNoAdapter);
    assert_eq!(result[0].message_id, message_id);
    assert_eq!(router.outbound_len(), 1);
}

#[test]
fn router_does_not_retry_adapter_errors_in_same_batch() {
    let adapter = Adapter::with_outbound_sender(|_message| {
        Err(LxmfError::Io("simulated adapter failure".into()))
    });
    let mut router = Router::with_adapter(adapter);
    router.set_auth_required(true);
    let first_destination = [0xC1; 16];
    let second_destination = [0xC2; 16];
    router.allow_destination(first_destination);
    router.allow_destination(second_destination);

    let first = make_message(first_destination, [0xD1; 16]);
    let second = make_message(second_destination, [0xD2; 16]);
    router.enqueue_outbound(first);
    router.enqueue_outbound(second);

    let result = router.handle_outbound(10).expect("outbound processing");
    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .all(|item| item.status == OutboundStatus::DeferredAdapterError));
    assert_eq!(router.stats().outbound_adapter_errors_total, 2);
    assert_eq!(router.outbound_len(), 2);
}
