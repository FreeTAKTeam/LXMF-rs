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
