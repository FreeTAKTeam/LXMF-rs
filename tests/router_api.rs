use lxmf::message::{Payload, WireMessage};
use lxmf::router::Router;

#[test]
fn router_can_queue_outbound() {
    let mut router = Router::default();
    let payload = Payload::new(1_700_000_000.0, Some("hi".into()), None, None);
    let msg = WireMessage::new([0u8; 16], [1u8; 16], payload);
    router.enqueue_outbound(msg);
    assert_eq!(router.outbound_len(), 1);
}
