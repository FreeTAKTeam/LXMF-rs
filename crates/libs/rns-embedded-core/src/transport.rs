use crate::{EmbeddedResult, packet::PacketFrame};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LinkState {
    Down,
    Connecting,
    Up,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TransportCaps {
    pub mtu_hint: u16,
    pub ordered_delivery: bool,
}

pub trait EmbeddedTransport {
    fn link_state(&self) -> LinkState;
    fn capabilities(&self) -> TransportCaps;
    fn send_frame(&mut self, frame: &PacketFrame) -> EmbeddedResult<()>;
    fn poll_frame(&mut self) -> EmbeddedResult<Option<PacketFrame>>;
}
