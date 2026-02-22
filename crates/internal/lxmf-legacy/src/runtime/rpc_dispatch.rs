use super::propagation_sync::request_messages_from_propagation_node_live;
use super::{
    annotate_peer_records_with_announce_metadata, annotate_response_meta,
    apply_runtime_identity_restore, RuntimeCommand, RuntimeResponse, WorkerState,
};
use serde_json::Value;

pub(super) async fn handle_runtime_request(
    state: &mut WorkerState,
    command: RuntimeCommand,
) -> Result<RuntimeResponse, String> {
    match command {
        RuntimeCommand::Status => {
            let mut status = state.status_template.clone();
            status.running = true;
            Ok(RuntimeResponse::Status(status))
        }
        RuntimeCommand::Call(request) => {
            let method = request.method.clone();
            let params_snapshot = request.params.clone();
            let mut result = if method == "request_messages_from_propagation_node"
                && state.transport.is_some()
            {
                request_messages_from_propagation_node_live(state, params_snapshot.as_ref())
                    .await
                    .map_err(|err| format!("rpc call failed: {err}"))?
            } else {
                let response = state
                    .daemon
                    .handle_rpc(request)
                    .map_err(|err| format!("rpc call failed: {err}"))?;
                if let Some(err) = response.error {
                    return Err(format!("rpc failed [{}]: {}", err.code, err.message));
                }
                response.result.unwrap_or(Value::Null)
            };
            if method == "list_peers" {
                let snapshot =
                    state.peer_announce_meta.lock().map(|guard| guard.clone()).unwrap_or_default();
                annotate_peer_records_with_announce_metadata(&mut result, &snapshot);
            }
            if method == "set_outbound_propagation_node" {
                let selected = result
                    .get("peer")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
                if let Ok(mut guard) = state.selected_propagation_node.lock() {
                    *guard = selected;
                }
            }
            if matches!(
                method.as_str(),
                "store_peer_identity"
                    | "restore_all_peer_identities"
                    | "bulk_restore_peer_identities"
                    | "bulk_restore_announce_identities"
            ) {
                apply_runtime_identity_restore(
                    &state.peer_crypto,
                    &state.peer_identity_cache_path,
                    method.as_str(),
                    params_snapshot.as_ref(),
                );
            }
            annotate_response_meta(&mut result, &state.profile, &state.status_template.rpc);
            Ok(RuntimeResponse::Value(result))
        }
        RuntimeCommand::PollEvent => Ok(RuntimeResponse::Event(state.daemon.take_event())),
        RuntimeCommand::Stop => {
            state.shutdown();
            Ok(RuntimeResponse::Ack)
        }
    }
}
