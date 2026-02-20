use std::collections::VecDeque;

use rns_transport::embedded_link::{
    EmbeddedLinkAdapter, EmbeddedLinkCapabilities, EmbeddedLinkError, EmbeddedLinkMedium,
};

#[derive(Debug)]
struct MockEmbeddedLink {
    adapter_id: &'static str,
    medium: EmbeddedLinkMedium,
    capabilities: EmbeddedLinkCapabilities,
    rx: VecDeque<Vec<u8>>,
    tx: VecDeque<Vec<u8>>,
}

impl MockEmbeddedLink {
    fn new(adapter_id: &'static str, medium: EmbeddedLinkMedium, mtu_bytes: usize) -> Self {
        Self {
            adapter_id,
            medium,
            capabilities: EmbeddedLinkCapabilities::new(mtu_bytes, true, true, true),
            rx: VecDeque::new(),
            tx: VecDeque::new(),
        }
    }

    fn enqueue_inbound(&mut self, frame: &[u8]) {
        self.rx.push_back(frame.to_vec());
    }
}

impl EmbeddedLinkAdapter for MockEmbeddedLink {
    fn adapter_id(&self) -> &str {
        self.adapter_id
    }

    fn medium(&self) -> EmbeddedLinkMedium {
        self.medium
    }

    fn capabilities(&self) -> EmbeddedLinkCapabilities {
        self.capabilities
    }

    fn send_frame(&mut self, frame: &[u8]) -> Result<(), EmbeddedLinkError> {
        if frame.len() > self.capabilities.mtu_bytes {
            return Err(EmbeddedLinkError::FrameTooLarge);
        }
        self.tx.push_back(frame.to_vec());
        Ok(())
    }

    fn poll_frame(&mut self) -> Result<Option<Vec<u8>>, EmbeddedLinkError> {
        Ok(self.rx.pop_front())
    }
}

#[test]
fn embedded_link_reports_stable_adapter_identity_and_capabilities() {
    let adapter = MockEmbeddedLink::new("serial-mock", EmbeddedLinkMedium::Serial, 96);
    let caps = adapter.capabilities();
    assert_eq!(adapter.adapter_id(), "serial-mock");
    assert_eq!(adapter.medium(), EmbeddedLinkMedium::Serial);
    assert_eq!(caps.mtu_bytes, 96);
    assert!(caps.supports_fragmentation);
}

#[test]
fn embedded_link_rejects_frames_over_mtu() {
    let mut adapter = MockEmbeddedLink::new("lora-mock", EmbeddedLinkMedium::Lora, 16);
    let oversized = vec![0u8; 32];
    let error = adapter.send_frame(&oversized).expect_err("oversized frame must fail");
    assert_eq!(error, EmbeddedLinkError::FrameTooLarge);
}

#[test]
fn embedded_link_poll_is_non_blocking_and_roundtrips_payload() {
    let mut adapter = MockEmbeddedLink::new("ble-mock", EmbeddedLinkMedium::BleGatt, 128);
    assert!(adapter.poll_frame().expect("poll succeeds").is_none());
    adapter.send_frame(b"hello").expect("send frame");
    assert_eq!(adapter.tx.pop_front().expect("tx frame"), b"hello".to_vec());

    adapter.enqueue_inbound(b"reply");
    assert_eq!(
        adapter.poll_frame().expect("poll succeeds").expect("inbound frame"),
        b"reply".to_vec()
    );
}
