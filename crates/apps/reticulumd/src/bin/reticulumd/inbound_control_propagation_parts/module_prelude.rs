use super::*;

#[cfg(test)]
use rns_transport::destination::DestinationName;

#[cfg(test)]
use sha2::Digest;

pub(super) fn handle_message_get_request(
    daemon: &RpcDaemon,
    remote_identity: &Identity,
    data: Option<rmpv::Value>,
    error_no_access: u8,
    error_invalid_data: u8,
) -> ControlResponse {
    shared_handler_response(lxmf_propagation::handlers::handle_get(
        &SharedGetBackend { daemon },
        &core_identity(remote_identity),
        data,
        error_no_access,
        error_invalid_data,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_offer_request(
    daemon: &RpcDaemon,
    control: &PropagationControlContext,
    link_id: &AddressHash,
    remote_identity: &Identity,
    data: Option<rmpv::Value>,
    error_no_access: u8,
    error_invalid_key: u8,
    error_invalid_data: u8,
    error_throttled: u8,
) -> ControlResponse {
    shared_handler_response(lxmf_propagation::handlers::handle_offer(
        &SharedOfferBackend { daemon, control, link_id },
        &control.local_identity_hash,
        &core_identity(remote_identity),
        data,
        error_no_access,
        error_invalid_key,
        error_invalid_data,
        error_throttled,
    ))
}

fn core_identity(identity: &Identity) -> rns_core::identity::Identity {
    rns_core::identity::Identity::new(identity.public_key, identity.verifying_key)
}
fn shared_handler_response(
    response: lxmf_propagation::handlers::HandlerResponse,
) -> ControlResponse {
    use lxmf_propagation::handlers::HandlerResponse;
    match response {
        HandlerResponse::Code(value) => ControlResponse::Code(value),
        HandlerResponse::Bool(value) => ControlResponse::Bool(value),
        HandlerResponse::Rmpv(value) => ControlResponse::Rmpv(value),
    }
}

struct SharedGetBackend<'a> {
    daemon: &'a RpcDaemon,
}
impl lxmf_propagation::handlers::GetBackend for SharedGetBackend<'_> {
    type Error = std::io::Error;
    fn delivery_identity_allowed(&self, identity: &rns_core::identity::Identity) -> bool {
        delivery_identity_allowed(
            self.daemon,
            &Identity::new(identity.public_key, identity.verifying_key),
        )
    }
    fn retain_synced_on_node(&self) -> bool {
        self.daemon.current_propagation_state().retain_synced_on_node
    }
    fn list_propagation_payloads_for_destination(
        &self,
        destination: &[u8; 16],
    ) -> Vec<(Vec<u8>, usize)> {
        self.daemon.list_propagation_payloads_for_destination(destination)
    }
    fn has_peer_completed_propagation_mark(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<bool, std::io::Error> {
        self.daemon.has_peer_completed_propagation_mark(peer, transient_id)
    }
    fn record_propagation_offer_peer(&self, peer: &str) -> Result<(), std::io::Error> {
        self.daemon.record_propagation_offer_peer(peer)
    }
    fn purge_propagation_payloads_for_destination(
        &self,
        destination: &[u8; 16],
        haves: &[Vec<u8>],
    ) -> usize {
        self.daemon.purge_propagation_payloads_for_destination(destination, haves)
    }
    fn has_peer_propagation_mark(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<bool, std::io::Error> {
        self.daemon.has_peer_propagation_mark(peer, transient_id)
    }
    fn record_existing_peer_received_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<bool, std::io::Error> {
        self.daemon.record_existing_peer_received_propagation(peer, transient_id)
    }
    fn preview_propagation_payloads_for_destination_with_ids(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> Vec<(String, Vec<u8>)> {
        self.daemon.preview_propagation_payloads_for_destination_with_ids(
            destination,
            wanted,
            transfer_limit_bytes,
        )
    }
    fn fetch_propagation_payloads_for_destination_with_ids(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> Vec<(String, Vec<u8>)> {
        self.daemon.fetch_propagation_payloads_for_destination_with_ids(
            destination,
            wanted,
            transfer_limit_bytes,
        )
    }
}

struct SharedOfferBackend<'a> {
    daemon: &'a RpcDaemon,
    control: &'a PropagationControlContext,
    link_id: &'a AddressHash,
}
impl lxmf_propagation::handlers::OfferBackend for SharedOfferBackend<'_> {
    type Error = std::io::Error;
    fn static_peer_allowed(&self, peer: &str) -> bool {
        let state = self.daemon.current_propagation_state();
        !state.from_static_only
            || state.static_peers.iter().any(|entry| entry.eq_ignore_ascii_case(peer))
    }
    fn peering_cost(&self) -> u32 {
        self.daemon.current_propagation_state().peering_cost.unwrap_or_else(|| {
            reticulum_daemon::announce_names::PropagationNodeAnnounceConfig::default().peering_cost
        })
    }
    fn mark_current_link_validated(&self) -> Result<(), Self::Error> {
        self.control
            .validated_peer_links
            .lock()
            .map_err(|error| {
                log::error!(
                    "[daemon-control] validated peer map lock poisoned while recording link={}: {}",
                    self.link_id,
                    error
                );
                std::io::Error::other(error.to_string())
            })?
            .insert(*self.link_id);
        Ok(())
    }
    fn propagation_peer_is_throttled(&self, peer: &str) -> bool {
        self.daemon.propagation_peer_is_throttled(peer)
    }
    fn propagation_peer_offer_is_throttled(&self, peer: &str) -> bool {
        self.daemon.propagation_peer_offer_is_throttled(peer)
    }
    fn throttle_propagation_peer_offer(&self, peer: &str) {
        self.daemon.throttle_propagation_peer_offer(peer)
    }
    fn has_propagation_payload(&self, transient_id: &str) -> bool {
        self.daemon.has_propagation_payload(transient_id)
    }
    fn record_peer_received_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<(), std::io::Error> {
        self.daemon.record_peer_received_propagation(peer, transient_id)
    }
    fn propagation_peer_admission_allowed(&self, peer: &str) -> bool {
        self.daemon.propagation_peer_admission_allowed(peer)
    }
}

#[cfg(test)]
fn delivery_destination_hash_for_identity(identity: &Identity) -> [u8; 16] {
    named_destination_hash_for_identity(identity, "delivery")
}

#[cfg(test)]
fn propagation_destination_hash_for_identity(identity: &Identity) -> [u8; 16] {
    named_destination_hash_for_identity(identity, "propagation")
}

fn delivery_identity_allowed(daemon: &RpcDaemon, identity: &Identity) -> bool {
    let remote_hash = hex::encode(identity.address_hash.as_slice());
    let response = match daemon.handle_rpc(RpcRequest {
        id: 0,
        method: "get_delivery_policy".to_string(),
        params: None,
    }) {
        Ok(response) => response,
        Err(error) => {
            log::error!(
                "[daemon-control] failed to load delivery policy identity={remote_hash}: {error}"
            );
            return false;
        }
    };
    if let Some(error) = response.error {
        log::error!(
            "[daemon-control] delivery policy RPC failed identity={} code={} error={}",
            remote_hash,
            error.code,
            error.message
        );
        return false;
    }
    let Some(policy) = response.result.and_then(|value| value.get("policy").cloned()) else {
        log::error!(
            "[daemon-control] delivery policy RPC returned no policy identity={remote_hash}"
        );
        return false;
    };
    if !policy.get("auth_required").and_then(Value::as_bool).unwrap_or(false) {
        return true;
    }
    policy.get("allowed_destinations").and_then(Value::as_array).is_some_and(|entries| {
        entries
            .iter()
            .filter_map(Value::as_str)
            .any(|entry| entry.eq_ignore_ascii_case(remote_hash.as_str()))
    })
}

#[cfg(test)]
fn named_destination_hash_for_identity(identity: &Identity, aspect: &str) -> [u8; 16] {
    let name = DestinationName::new("lxmf", aspect);
    let hash = sha2::Sha256::new()
        .chain_update(name.as_name_hash_slice())
        .chain_update(identity.address_hash.as_slice())
        .finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&hash[..16]);
    out
}
