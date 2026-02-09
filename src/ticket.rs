#[derive(Debug, Clone, PartialEq)]
pub struct Ticket {
    pub expires: f64,
    pub token: Vec<u8>,
}

pub const TICKET_EXPIRY: f64 = 21.0 * 24.0 * 60.0 * 60.0;
pub const TICKET_GRACE: f64 = 5.0 * 24.0 * 60.0 * 60.0;
pub const TICKET_RENEW: f64 = 14.0 * 24.0 * 60.0 * 60.0;
pub const TICKET_INTERVAL: f64 = 1.0 * 24.0 * 60.0 * 60.0;
pub const COST_TICKET: u32 = 0x100;

impl Ticket {
    pub fn new(expires: f64, token: Vec<u8>) -> Self {
        Self { expires, token }
    }

    pub fn is_valid(&self, now: f64) -> bool {
        now <= self.expires
    }

    pub fn is_valid_with_grace(&self, now: f64) -> bool {
        now <= self.expires + TICKET_GRACE
    }

    pub fn needs_renewal(&self, now: f64) -> bool {
        self.expires - now <= TICKET_RENEW
    }

    pub fn stamp_for_message(&self, message_id: &[u8]) -> Vec<u8> {
        let mut material = Vec::with_capacity(self.token.len() + message_id.len());
        material.extend_from_slice(&self.token);
        material.extend_from_slice(message_id);
        let digest = reticulum::hash::Hash::new_from_slice(&material).to_bytes();
        digest[..crate::constants::TICKET_LENGTH].to_vec()
    }
}
