use super::*;

impl Router {
    pub fn register_identity(&mut self, destination: [u8; 16], name: Option<String>) -> bool {
        self.registered_identities.insert(destination, name).is_none()
    }

    pub fn unregister_identity(&mut self, destination: &[u8; 16]) -> bool {
        self.registered_identities.remove(destination).is_some()
    }

    pub fn is_identity_registered(&self, destination: &[u8; 16]) -> bool {
        self.registered_identities.contains_key(destination)
    }

    pub fn identity_name(&self, destination: &[u8; 16]) -> Option<&str> {
        self.registered_identities.get(destination).and_then(|n| n.as_deref())
    }

    pub fn register_delivery_callback(&mut self, callback: DeliveryCallback) {
        self.delivery_callbacks.push(callback);
    }

    pub fn register_outbound_progress_callback(&mut self, callback: OutboundProgressCallback) {
        self.outbound_progress_callbacks.push(callback);
    }

    pub fn set_auth_required(&mut self, enabled: bool) {
        self.config.auth_required = enabled;
    }

    pub fn auth_required(&self) -> bool {
        self.config.auth_required
    }

    pub fn allow_destination(&mut self, destination: [u8; 16]) {
        self.allowed_destinations.insert(destination);
        self.denied_destinations.remove(&destination);
    }

    pub fn deny_destination(&mut self, destination: [u8; 16]) {
        self.denied_destinations.insert(destination);
        self.allowed_destinations.remove(&destination);
    }

    pub fn clear_destination_policy(&mut self, destination: &[u8; 16]) {
        self.allowed_destinations.remove(destination);
        self.denied_destinations.remove(destination);
    }

    pub fn is_destination_allowed(&self, destination: &[u8; 16]) -> bool {
        if self.denied_destinations.contains(destination) {
            return false;
        }

        if !self.config.auth_required {
            return true;
        }

        if self.allowed_destinations.contains(destination) {
            return true;
        }

        self.registered_identities.contains_key(destination)
    }

    pub fn ignore_destination(&mut self, destination: [u8; 16]) {
        self.ignored_destinations.insert(destination);
    }

    pub fn unignore_destination(&mut self, destination: &[u8; 16]) {
        self.ignored_destinations.remove(destination);
    }

    pub fn is_destination_ignored(&self, destination: &[u8; 16]) -> bool {
        self.ignored_destinations.contains(destination)
    }

    pub fn prioritise_destination(&mut self, destination: [u8; 16]) {
        self.prioritised_destinations.insert(destination);
    }

    pub fn deprioritise_destination(&mut self, destination: &[u8; 16]) {
        self.prioritised_destinations.remove(destination);
    }

    pub fn is_destination_prioritised(&self, destination: &[u8; 16]) -> bool {
        self.prioritised_destinations.contains(destination)
    }
}
