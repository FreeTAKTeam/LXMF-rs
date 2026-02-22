use crate::error::LxmfError;
use crate::helpers::{pn_name_from_app_data, pn_stamp_cost_from_app_data};
use crate::helpers::{pn_peering_cost_from_app_data, pn_stamp_cost_flexibility_from_app_data};
use crate::router::Router;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropagationAnnounceEvent {
    pub destination: [u8; 16],
    pub name: Option<String>,
    pub stamp_cost: Option<u32>,
    pub stamp_cost_flexibility: Option<u32>,
    pub peering_cost: Option<u32>,
}

type DeliveryCallback = Box<dyn FnMut(&[u8; 16]) -> Result<(), LxmfError> + Send + Sync + 'static>;
type PropagationCallback =
    Box<dyn FnMut(&PropagationAnnounceEvent) -> Result<(), LxmfError> + Send + Sync + 'static>;

pub struct DeliveryAnnounceHandler {
    callback: Option<DeliveryCallback>,
}

impl DeliveryAnnounceHandler {
    pub fn new() -> Self {
        Self { callback: None }
    }

    pub fn with_callback(callback: DeliveryCallback) -> Self {
        Self { callback: Some(callback) }
    }

    pub fn handle(&mut self, dest: &[u8; 16]) -> Result<(), LxmfError> {
        if let Some(callback) = &mut self.callback {
            callback(dest)?;
        }
        Ok(())
    }

    pub fn handle_with_router(
        &mut self,
        router: &mut Router,
        dest: &[u8; 16],
    ) -> Result<(), LxmfError> {
        router.register_identity(*dest, None);
        router.allow_destination(*dest);
        router.register_peer(*dest);
        if let Some(peer) = router.peer_mut(dest) {
            peer.mark_seen(unix_now());
        }
        self.handle(dest)
    }
}

impl Default for DeliveryAnnounceHandler {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PropagationAnnounceHandler {
    callback: Option<PropagationCallback>,
}

impl PropagationAnnounceHandler {
    pub fn new() -> Self {
        Self { callback: None }
    }

    pub fn with_callback(callback: PropagationCallback) -> Self {
        Self { callback: Some(callback) }
    }

    pub fn handle(&mut self, dest: &[u8; 16]) -> Result<(), LxmfError> {
        let event = PropagationAnnounceEvent {
            destination: *dest,
            name: None,
            stamp_cost: None,
            stamp_cost_flexibility: None,
            peering_cost: None,
        };
        if let Some(callback) = &mut self.callback {
            callback(&event)?;
        }
        Ok(())
    }

    pub fn handle_with_app_data(
        &mut self,
        dest: &[u8; 16],
        app_data: &[u8],
    ) -> Result<PropagationAnnounceEvent, LxmfError> {
        let event = PropagationAnnounceEvent {
            destination: *dest,
            name: pn_name_from_app_data(app_data),
            stamp_cost: pn_stamp_cost_from_app_data(app_data),
            stamp_cost_flexibility: pn_stamp_cost_flexibility_from_app_data(app_data),
            peering_cost: pn_peering_cost_from_app_data(app_data),
        };

        if let Some(callback) = &mut self.callback {
            callback(&event)?;
        }

        Ok(event)
    }

    pub fn handle_with_router(
        &mut self,
        router: &mut Router,
        dest: &[u8; 16],
        app_data: &[u8],
    ) -> Result<PropagationAnnounceEvent, LxmfError> {
        let event = self.handle_with_app_data(dest, app_data)?;
        router.register_identity(*dest, event.name.clone());
        router.allow_destination(*dest);
        router.prioritise_destination(*dest);
        router.register_peer(*dest);
        if let Some(peer) = router.peer_mut(dest) {
            peer.mark_seen(unix_now());
            if let Some(name) = &event.name {
                peer.set_name(name.clone());
            }
        }
        Ok(event)
    }
}

impl Default for PropagationAnnounceHandler {
    fn default() -> Self {
        Self::new()
    }
}

fn unix_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}
