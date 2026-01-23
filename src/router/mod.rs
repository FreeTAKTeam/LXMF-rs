use crate::message::WireMessage;
use crate::reticulum::Adapter;

#[derive(Default)]
pub struct Router {
    outbound: Vec<WireMessage>,
}

impl Router {
    pub fn with_adapter(_adapter: Adapter) -> Self {
        Self::default()
    }

    pub fn enqueue_outbound(&mut self, msg: WireMessage) {
        self.outbound.push(msg);
    }

    pub fn outbound_len(&self) -> usize {
        self.outbound.len()
    }
}
