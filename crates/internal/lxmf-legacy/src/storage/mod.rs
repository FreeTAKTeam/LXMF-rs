mod file_store;
mod propagation_store;

use crate::message::WireMessage;
pub use file_store::FileStore;
pub use propagation_store::PropagationStore;

pub trait Store {
    fn save(&self, msg: &WireMessage) -> Result<(), crate::error::LxmfError>;
    fn get(&self, id: &[u8; 32]) -> Result<WireMessage, crate::error::LxmfError>;
}
