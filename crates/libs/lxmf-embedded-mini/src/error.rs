#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MiniError {
    InvalidInput,
    InvalidWire,
    BufferTooSmall,
    CapacityExceeded,
    Backpressure,
    Disconnected,
    ReplayRejected,
    EventOverflow,
    Unsupported,
}

pub type MiniResult<T> = core::result::Result<T, MiniError>;
