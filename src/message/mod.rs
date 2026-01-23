mod payload;
mod wire;

pub use payload::Payload;
pub use wire::WireMessage;

#[derive(Default, Debug, Clone)]
pub struct Message;
