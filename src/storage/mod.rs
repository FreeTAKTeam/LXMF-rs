mod file_store;

pub use file_store::FileStore;
use crate::message::WireMessage;

pub trait Store {
    fn save(&self, msg: &WireMessage) -> Result<(), crate::error::LxmfError>;
    fn get(&self, id: &[u8; 32]) -> Result<WireMessage, crate::error::LxmfError>;
}
