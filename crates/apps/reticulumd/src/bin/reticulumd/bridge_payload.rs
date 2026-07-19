use reticulum_daemon::lxmf_bridge::build_wire_message_with_options_and_cancel;
use rns_core::identity::PrivateIdentity;
use rns_rpc::RpcDaemon;
use serde_json::Value as JsonValue;
use std::sync::Arc;

pub(super) struct OutboundPayloadBuild {
    pub(super) daemon: Arc<RpcDaemon>,
    pub(super) message_id: String,
    pub(super) source_hash: [u8; 16],
    pub(super) destination: [u8; 16],
    pub(super) title: String,
    pub(super) content: String,
    pub(super) fields: Option<JsonValue>,
    pub(super) signer: PrivateIdentity,
    pub(super) stamp_cost: Option<u32>,
    pub(super) outbound_ticket: Option<String>,
    pub(super) include_ticket: Option<(i64, Vec<u8>)>,
}

pub(super) async fn build_outbound_payload(
    input: OutboundPayloadBuild,
) -> Result<Vec<u8>, std::io::Error> {
    tokio::task::spawn_blocking(move || {
        let status_lookup_error = std::sync::Mutex::new(None);
        let result = build_wire_message_with_options_and_cancel(
            input.source_hash,
            input.destination,
            input.title.as_str(),
            input.content.as_str(),
            input.fields,
            &input.signer,
            input.stamp_cost,
            input.outbound_ticket.as_deref(),
            input
                .include_ticket
                .as_ref()
                .map(|(expires_at, ticket)| (*expires_at, ticket.as_slice())),
            || match input.daemon.message_receipt_status(&input.message_id) {
                Ok(status) => {
                    status.is_some_and(|status| status.trim().eq_ignore_ascii_case("cancelled"))
                }
                Err(error) => {
                    let mut guard = status_lookup_error
                        .lock()
                        .expect("outbound status lookup error mutex poisoned");
                    if guard.is_none() {
                        *guard = Some(error.to_string());
                    }
                    true
                }
            },
        );
        if let Some(error) =
            status_lookup_error.into_inner().expect("outbound status lookup error mutex poisoned")
        {
            return Err(std::io::Error::other(format!(
                "receipt status lookup failed for message {}: {error}",
                input.message_id
            )));
        }
        result.map_err(std::io::Error::other)
    })
    .await
    .map_err(std::io::Error::other)?
}
