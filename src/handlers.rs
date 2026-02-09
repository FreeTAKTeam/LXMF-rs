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

impl Default for DeliveryAnnounceHandler {
    fn default() -> Self {
        Self::new()
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

impl Default for PropagationAnnounceHandler {
    fn default() -> Self {
        Self::new()
    }
}
