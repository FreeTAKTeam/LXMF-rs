use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedLinkMedium {
    Serial,
    BleGatt,
    Lora,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedLinkCapabilities {
    pub mtu_bytes: usize,
    pub supports_fragmentation: bool,
    pub supports_ordered_delivery: bool,
    pub supports_ack: bool,
}

impl EmbeddedLinkCapabilities {
    pub const fn new(
        mtu_bytes: usize,
        supports_fragmentation: bool,
        supports_ordered_delivery: bool,
        supports_ack: bool,
    ) -> Self {
        Self { mtu_bytes, supports_fragmentation, supports_ordered_delivery, supports_ack }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedLinkConfig {
    pub adapter_id: String,
    pub medium: EmbeddedLinkMedium,
    pub max_queue_depth: usize,
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddedLinkError {
    NotReady,
    QueueFull,
    FrameTooLarge,
    Io,
    Other(String),
}

pub trait EmbeddedLinkAdapter {
    fn adapter_id(&self) -> &str;
    fn medium(&self) -> EmbeddedLinkMedium;
    fn capabilities(&self) -> EmbeddedLinkCapabilities;
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), EmbeddedLinkError>;
    fn poll_frame(&mut self) -> Result<Option<Vec<u8>>, EmbeddedLinkError>;
}

#[cfg(test)]
mod tests {
    use super::{
        EmbeddedLinkAdapter, EmbeddedLinkCapabilities, EmbeddedLinkConfig, EmbeddedLinkError,
        EmbeddedLinkMedium,
    };
    use alloc::collections::VecDeque;
    use alloc::vec::Vec;

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
    fn embedded_link_config_tracks_medium_and_queue_policy() {
        let config = EmbeddedLinkConfig {
            adapter_id: "ble-mock".to_string(),
            medium: EmbeddedLinkMedium::BleGatt,
            max_queue_depth: 32,
            poll_interval_ms: 10,
        };
        assert_eq!(config.medium, EmbeddedLinkMedium::BleGatt);
        assert_eq!(config.max_queue_depth, 32);
    }

    #[test]
    fn mock_embedded_link_roundtrip_conformance() {
        let mut adapter = MockEmbeddedLink::new("serial-mock", EmbeddedLinkMedium::Serial, 128);

        adapter.send_frame(b"hello").expect("send frame");
        assert_eq!(adapter.tx.pop_front().expect("tx frame"), b"hello".to_vec());

        adapter.enqueue_inbound(b"reply");
        assert_eq!(adapter.poll_frame().expect("poll").expect("frame"), b"reply".to_vec());
        assert!(adapter.poll_frame().expect("poll").is_none());
    }

    #[test]
    fn mock_embedded_link_rejects_oversized_frames() {
        let mut adapter = MockEmbeddedLink::new("lora-mock", EmbeddedLinkMedium::Lora, 16);
        let oversized = vec![0u8; 32];
        let err = adapter.send_frame(&oversized).expect_err("frame must be rejected");
        assert_eq!(err, EmbeddedLinkError::FrameTooLarge);
    }
}
