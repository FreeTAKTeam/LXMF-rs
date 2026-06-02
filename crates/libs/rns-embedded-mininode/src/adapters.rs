use alloc::{collections::VecDeque, vec::Vec};

use crate::error::MiniNodeError;

pub trait FrameLink {
    fn is_up(&self) -> bool;
    fn mtu(&self) -> usize;
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), MiniNodeError>;
    fn poll_frame(&mut self) -> Result<Option<Vec<u8>>, MiniNodeError>;
}

#[derive(Debug, Clone)]
pub struct MemoryLink {
    mtu: usize,
    online: bool,
    inbound: VecDeque<Vec<u8>>,
    outbound: VecDeque<Vec<u8>>,
}

impl Default for MemoryLink {
    fn default() -> Self {
        Self::new(512)
    }
}

impl MemoryLink {
    pub fn new(mtu: usize) -> Self {
        Self { mtu, online: true, inbound: VecDeque::new(), outbound: VecDeque::new() }
    }

    pub fn set_online(&mut self, online: bool) {
        self.online = online;
    }

    pub fn push_inbound(&mut self, frame: Vec<u8>) {
        self.inbound.push_back(frame);
    }

    pub fn drain_outbound(&mut self) -> Vec<Vec<u8>> {
        self.outbound.drain(..).collect()
    }
}

impl FrameLink for MemoryLink {
    fn is_up(&self) -> bool {
        self.online
    }

    fn mtu(&self) -> usize {
        self.mtu
    }

    fn send_frame(&mut self, frame: &[u8]) -> Result<(), MiniNodeError> {
        if !self.online {
            return Err(MiniNodeError::LinkDown);
        }
        if frame.len() > self.mtu {
            return Err(MiniNodeError::MtuExceeded { frame_len: frame.len(), mtu: self.mtu });
        }
        self.outbound.push_back(frame.to_vec());
        Ok(())
    }

    fn poll_frame(&mut self) -> Result<Option<Vec<u8>>, MiniNodeError> {
        if !self.online {
            return Ok(None);
        }
        Ok(self.inbound.pop_front())
    }
}
