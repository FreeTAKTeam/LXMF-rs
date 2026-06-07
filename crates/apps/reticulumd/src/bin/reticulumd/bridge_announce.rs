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
        let Some((display_name, _)) = parse_peer_name_from_app_data(app_data.as_slice()) else {
            return Some(app_data);
        };
        encode_delivery_announce_app_data_with_capabilities(
            display_name.as_str(),
            self.current_inbound_stamp_cost(),
            &self.announce_capabilities,
        )
        .or(Some(app_data))
    }

    fn current_inbound_stamp_cost(&self) -> Option<u32> {
        let daemon = self.daemon.lock().ok()?.clone()?;
        let target_cost = daemon.current_stamp_policy().target_cost;
        (target_cost > 0 && target_cost < 255).then_some(target_cost)
    }

    fn current_propagation_announce_app_data(&self) -> Option<Vec<u8>> {
        let fallback = self.propagation_announce_app_data.clone()?;
        let display_name = parse_peer_name_from_app_data(fallback.as_slice()).map(|(name, _)| name);
        let Some(daemon) = self.daemon.lock().ok()?.clone() else {
            return Some(fallback);
        };
        let state = daemon.current_propagation_state();
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
        .or(Some(fallback))
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
}
