use crate::error::LxmfError;

pub struct DeliveryAnnounceHandler;

impl DeliveryAnnounceHandler {
    pub fn new() -> Self {
        Self
    }

    pub fn handle(&mut self, _dest: &[u8; 16]) -> Result<(), LxmfError> {
        Ok(())
    }
}

pub struct PropagationAnnounceHandler;

impl PropagationAnnounceHandler {
    pub fn new() -> Self {
        Self
    }

    pub fn handle(&mut self, _dest: &[u8; 16]) -> Result<(), LxmfError> {
        Ok(())
    }
}
