use super::*;
use reticulum_daemon::announce_names::{
    encode_delivery_announce_app_data_with_capabilities,
    encode_propagation_node_app_data as encode_python_propagation_node_app_data,
    parse_peer_name_from_app_data, PropagationNodeAnnounceConfig,
};
use rns_rpc::AnnounceBridge;

impl TransportBridge {
    fn current_delivery_announce_app_data(&self) -> Option<Vec<u8>> {
        let app_data = self.announce_app_data.clone()?;
        if app_data.is_empty() {
            return Some(app_data);
        }
        let Some((display_name, _)) = (match parse_peer_name_from_app_data(app_data.as_slice()) {
            Ok(opt) => opt,
            Err(e) => {
                log::warn!("[daemon] failed to parse peer name from delivery app data: {e}");
                None
            }
        }) else {
            return Some(app_data);
        };
        Some(
            encode_delivery_announce_app_data_with_capabilities(
                display_name.as_str(),
                self.current_inbound_stamp_cost().unwrap_or_else(|err| {
                    log::warn!("[daemon] failed to read daemon state for delivery announce: {err}");
                    None
                }),
                &self.announce_capabilities,
            )
            .unwrap_or_else(|e| {
                log::warn!("[daemon] failed to encode delivery announce app data: {e}");
                app_data
            }),
        )
    }

    fn current_inbound_stamp_cost(&self) -> Result<Option<u32>, &'static str> {
        let daemon = match self.daemon.lock() {
            Ok(daemon) => daemon.clone(),
            Err(_) => return Err("daemon state lock poisoned"),
        };
        let Some(daemon) = daemon else {
            return Ok(None);
        };
        let target_cost = daemon.current_stamp_policy().target_cost;
        Ok((target_cost > 0 && target_cost < 255).then_some(target_cost))
    }

    fn current_propagation_announce_app_data(&self) -> Option<Vec<u8>> {
        let fallback = self.propagation_announce_app_data.clone()?;
        let display_name = if fallback.is_empty() {
            None
        } else {
            match parse_peer_name_from_app_data(fallback.as_slice()) {
                Ok(opt) => opt.map(|(name, _)| name),
                Err(e) => {
                    log::warn!("[daemon] failed to parse peer name from propagation app data: {e}");
                    None
                }
            }
        };
        let daemon = match self.daemon.lock() {
            Ok(daemon) => {
                let Some(daemon) = daemon.clone() else {
                    return Some(fallback);
                };
                daemon
            }
            Err(err) => {
                log::warn!("[daemon] failed to read daemon state for propagation announce: {err}");
                return Some(fallback);
            }
        };
        let state = daemon.current_propagation_state();
        Some(
            encode_python_propagation_node_app_data(
                display_name.as_deref(),
                PropagationNodeAnnounceConfig {
                    enabled: state.enabled,
                    timebase: now_secs_i64(),
                    transfer_limit_kb: state.propagation_limit,
                    sync_limit_kb: state.sync_limit,
                    stamp_cost: if state.target_cost > 0 {
                        state.target_cost
                    } else {
                        PropagationNodeAnnounceConfig::default().stamp_cost
                    },
                    stamp_cost_flexibility: state.stamp_cost_flexibility,
                    peering_cost: state
                        .peering_cost
                        .unwrap_or_else(|| PropagationNodeAnnounceConfig::default().peering_cost),
                },
            )
            .unwrap_or_else(|e| {
                log::warn!("[daemon] failed to encode propagation announce app data: {e}");
                fallback
            }),
        )
    }

    #[cfg(test)]
    pub(crate) fn current_propagation_announce_app_data_for_test(&self) -> Option<Vec<u8>> {
        self.current_propagation_announce_app_data()
    }

    pub(crate) fn announce_propagation_now(&self) -> Result<(), std::io::Error> {
        let transport = self.transport.clone();
        let propagation_destination = self.propagation_announce_destination.clone();
        let propagation_app_data = self.current_propagation_announce_app_data();
        let control_destination = self.control_announce_destination.clone();
        tokio::spawn(async move {
            if let Some(destination) = propagation_destination.as_ref() {
                transport
                    .set_destination_announce_app_data(destination, propagation_app_data.clone())
                    .await;
                transport.send_announce(destination, propagation_app_data.as_deref()).await;
            }
            if let Some(destination) = control_destination.as_ref() {
                transport.send_announce(destination, None).await;
            }
        });
        Ok(())
    }
}

impl AnnounceBridge for TransportBridge {
    fn announce_now(&self) -> Result<(), std::io::Error> {
        let transport = self.transport.clone();
        let destination = self.announce_destination.clone();
        let app_data = self.current_delivery_announce_app_data();
        let propagation_destination = self.propagation_announce_destination.clone();
        let propagation_app_data = self.current_propagation_announce_app_data();
        let control_destination = self.control_announce_destination.clone();
        tokio::spawn(async move {
            transport.set_destination_announce_app_data(&destination, app_data.clone()).await;
            transport.send_announce(&destination, app_data.as_deref()).await;
            if let Some(destination) = propagation_destination.as_ref() {
                transport
                    .set_destination_announce_app_data(destination, propagation_app_data.clone())
                    .await;
                transport.send_announce(destination, propagation_app_data.as_deref()).await;
            }
            if let Some(destination) = control_destination.as_ref() {
                transport.send_announce(destination, None).await;
            }
        });
        Ok(())
    }

    fn announce_delivery(&self, destination_hash: &str) -> Result<(), std::io::Error> {
        self.announce_registered_service_destination(destination_hash)
    }
}
