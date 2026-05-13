use crate::MiniResult;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LinkState {
    Down,
    Connecting,
    Up,
}

pub trait MiniTransport {
    fn link_state(&self) -> LinkState;
    fn send_frame(&mut self, frame: &[u8]) -> MiniResult<()>;
    fn poll_frame(&mut self, out: &mut [u8]) -> MiniResult<Option<usize>>;
}
