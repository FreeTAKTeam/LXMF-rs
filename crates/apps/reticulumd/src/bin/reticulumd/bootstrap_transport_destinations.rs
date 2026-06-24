use super::{encode_propagation_node_app_data, pretty_daemon_line};
use reticulum_daemon::announce_names::encode_delivery_announce_app_data_with_capabilities;
use reticulum_daemon::announce_names::PropagationNodeAnnounceConfig;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::identity::PrivateIdentity;
use rns_transport::transport::Transport;
use std::sync::Arc;

pub(super) struct RegisteredTransportDestinations {
    pub(super) delivery: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    pub(super) propagation: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    pub(super) control: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    pub(super) delivery_destination_hash_hex: String,
    pub(super) propagation_destination_hash_hex: Option<String>,
    pub(super) control_destination_hash_hex: Option<String>,
    pub(super) delivery_source_hash: [u8; 16],
}

pub(super) async fn register_transport_destinations(
    transport: &mut Transport,
    transport_identity: PrivateIdentity,
    local_display_name: Option<&str>,
    local_announce_capabilities: &[String],
    propagation_announce_app_data: Option<Vec<u8>>,
    propagation_control_enabled: bool,
    propagation_announce_config: PropagationNodeAnnounceConfig,
) -> RegisteredTransportDestinations {
    let delivery = transport
        .add_destination(transport_identity.clone(), DestinationName::new("lxmf", "delivery"))
        .await;
    let (delivery_source_hash, delivery_destination_hash_hex) =
        destination_hash("delivery", &delivery).await;
    transport
        .set_destination_announce_app_data(
            &delivery,
            local_display_name.and_then(|display_name| {
                encode_delivery_announce_app_data_with_capabilities(
                    display_name,
                    None,
                    local_announce_capabilities,
                )
                .inspect_err(|e| {
                    log::warn!("[daemon] failed to encode delivery announce app data: {e}")
                })
                .ok()
            }),
        )
        .await;

    let mut propagation = None;
    let mut control = None;
    let mut propagation_destination_hash_hex = None;
    let mut control_destination_hash_hex = None;
    if propagation_control_enabled {
        let propagation_destination = transport
            .add_destination(
                transport_identity.clone(),
                DestinationName::new("lxmf", "propagation"),
            )
            .await;
        let (_, hash_hex) = destination_hash("propagation", &propagation_destination).await;
        propagation_destination_hash_hex = Some(hash_hex);
        transport
            .set_destination_announce_app_data(
                &propagation_destination,
                propagation_announce_app_data.clone().or_else(|| {
                    encode_propagation_node_app_data(
                        local_display_name,
                        propagation_announce_config,
                    )
                    .inspect_err(|e| {
                        log::warn!("[daemon] failed to encode propagation announce app data: {e}")
                    })
                    .ok()
                }),
            )
            .await;
        propagation = Some(propagation_destination);

        let control_destination = transport
            .add_destination(
                transport_identity,
                DestinationName::new("lxmf", "propagation.control"),
            )
            .await;
        let (_, hash_hex) = destination_hash("control", &control_destination).await;
        control_destination_hash_hex = Some(hash_hex);
        control = Some(control_destination);
    }

    RegisteredTransportDestinations {
        delivery,
        propagation,
        control,
        delivery_destination_hash_hex,
        propagation_destination_hash_hex,
        control_destination_hash_hex,
        delivery_source_hash,
    }
}

async fn destination_hash(
    label: &str,
    destination: &Arc<tokio::sync::Mutex<SingleInputDestination>>,
) -> ([u8; 16], String) {
    let dest = destination.lock().await;
    let mut hash = [0u8; 16];
    hash.copy_from_slice(dest.desc.address_hash.as_slice());
    let hash_hex = hex::encode(hash);
    println!("{}", daemon_destination_hash_line(label, hash_hex.as_str()));
    (hash, hash_hex)
}

fn daemon_destination_hash_line(label: &str, hash_hex: &str) -> String {
    pretty_daemon_line(&format!("{label} destination hash={hash_hex}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_hash_line_keeps_smoke_script_marker() {
        let line = daemon_destination_hash_line("delivery", "0123456789abcdef");

        assert!(line.contains("delivery destination hash=0123456789abcdef"));
    }
}
