use super::inbound_helpers::decode_inbound_payload;
use super::propagation_link::{
    build_link_identify_payload, build_link_request_payload, parse_binary_array,
    propagation_error_from_response_value, send_link_context_packet,
    wait_for_link_request_response,
};
use super::{
    annotate_peer_records_with_announce_metadata, annotate_response_meta,
    apply_runtime_identity_restore, clean_non_empty, normalize_relay_destination_hash,
    now_epoch_secs, parse_destination_hex, update_runtime_propagation_sync_state, RuntimeCommand,
    RuntimePropagationSyncParams, RuntimeResponse, WorkerState, PROPAGATION_LINK_TIMEOUT,
    PROPAGATION_PATH_TIMEOUT, PROPAGATION_REQUEST_TIMEOUT, PR_COMPLETE, PR_IDLE,
    PR_LINK_ESTABLISHED, PR_LINK_ESTABLISHING, PR_LINK_FAILED, PR_NO_PATH, PR_PATH_REQUESTED,
    PR_RECEIVING, PR_REQUEST_SENT, PR_RESPONSE_RECEIVED,
};
use crate::inbound_decode::InboundPayloadMode;
use reticulum::delivery::await_link_activation;
use reticulum::destination::{DestinationName, SingleOutputDestination};
use reticulum::hash::{AddressHash, Hash};
use reticulum::identity::PrivateIdentity;
use reticulum::packet::PacketContext;
use reticulum::rpc::RpcRequest;
use serde_json::{json, Value};
use std::time::Duration;

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

async fn request_messages_from_propagation_node_live(
    state: &WorkerState,
    params: Option<&Value>,
) -> Result<Value, std::io::Error> {
    let parsed = params
        .map(|value| serde_json::from_value::<RuntimePropagationSyncParams>(value.clone()))
        .transpose()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
        .unwrap_or_default();
    let max_messages = parsed.max_messages.unwrap_or(256).clamp(1, 4096);
    let max_messages_usize = max_messages as usize;
    let started_at = now_epoch_secs() as i64;

    let selected_node = state
        .selected_propagation_node
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let Some(selected_node) = selected_node else {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = PR_IDLE;
            guard.state_name = "idle".to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.max_messages = max_messages;
            guard.selected_node = None;
            guard.last_sync_started = Some(completed);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some("No propagation node configured".to_string());
        });
        return Ok(json!({
            "success": false,
            "error": "No propagation node configured",
            "errorCode": "NO_PROPAGATION_NODE",
            "state": PR_IDLE,
            "state_name": "idle",
            "progress": 0.0_f64,
            "messages_received": 0_u32,
        }));
    };

    let request_identity = if let Some(raw) =
        parsed.identity_private_key.and_then(|value| clean_non_empty(Some(value)))
    {
        let bytes = hex::decode(raw.trim()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "identity_private_key must be hex-encoded",
            )
        })?;
        PrivateIdentity::from_private_key_bytes(&bytes).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "identity_private_key is not a valid identity private key",
            )
        })?
    } else {
        state.local_identity.clone()
    };

    let Some(transport) = state.transport.clone() else {
        return Err(std::io::Error::other("embedded transport unavailable"));
    };

    let relay_peer = normalize_relay_destination_hash(&state.peer_crypto, &selected_node)
        .unwrap_or(selected_node.clone());
    let Some(relay_destination) = parse_destination_hex(&relay_peer) else {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = PR_NO_PATH;
            guard.state_name = "no_path".to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.max_messages = max_messages;
            guard.selected_node = Some(selected_node.clone());
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some("Invalid propagation node hash".to_string());
        });
        return Ok(json!({
            "success": false,
            "error": "Invalid propagation node hash",
            "errorCode": "INVALID_NODE_HASH",
            "state": PR_NO_PATH,
            "state_name": "no_path",
            "progress": 0.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
        }));
    };
    let relay_hash = AddressHash::new(relay_destination);

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_PATH_REQUESTED;
        guard.state_name = "path_requested".to_string();
        guard.sync_progress = 0.05;
        guard.messages_received = 0;
        guard.max_messages = max_messages;
        guard.selected_node = Some(selected_node.clone());
        guard.last_sync_started = Some(started_at);
        guard.last_sync_completed = None;
        guard.last_sync_error = None;
    });

    let mut relay_identity = transport.destination_identity(&relay_hash).await;
    if relay_identity.is_none() {
        transport.request_path(&relay_hash, None, None).await;
        let deadline = tokio::time::Instant::now() + PROPAGATION_PATH_TIMEOUT;
        while relay_identity.is_none() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(250)).await;
            relay_identity = transport.destination_identity(&relay_hash).await;
        }
    }
    let Some(relay_identity) = relay_identity else {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = PR_NO_PATH;
            guard.state_name = "no_path".to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.max_messages = max_messages;
            guard.selected_node = Some(selected_node.clone());
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some("No path known for propagation node".to_string());
        });
        return Ok(json!({
            "success": false,
            "error": "No path known for propagation node",
            "errorCode": "NO_PATH",
            "state": PR_NO_PATH,
            "state_name": "no_path",
            "progress": 0.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
        }));
    };

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_LINK_ESTABLISHING;
        guard.state_name = "link_establishing".to_string();
        guard.sync_progress = 0.2;
    });

    let relay_destination =
        SingleOutputDestination::new(relay_identity, DestinationName::new("lxmf", "propagation"));
    let link = transport.link(relay_destination.desc).await;
    if let Err(err) = await_link_activation(&transport, &link, PROPAGATION_LINK_TIMEOUT).await {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = PR_LINK_FAILED;
            guard.state_name = "link_failed".to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some(err.to_string());
        });
        return Ok(json!({
            "success": false,
            "error": err.to_string(),
            "errorCode": "LINK_FAILED",
            "state": PR_LINK_FAILED,
            "state_name": "link_failed",
            "progress": 0.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
        }));
    }

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_LINK_ESTABLISHED;
        guard.state_name = "link_established".to_string();
        guard.sync_progress = 0.35;
    });

    let link_id = *link.lock().await.id();
    let identify_payload = build_link_identify_payload(&request_identity, &link_id);
    send_link_context_packet(
        &transport,
        &link,
        PacketContext::LinkIdentify,
        identify_payload.as_slice(),
    )
    .await?;

    let mut data_rx = transport.received_data_events();
    let mut resource_rx = transport.resource_events();

    let list_payload = build_link_request_payload(
        "/get",
        rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil]),
    )?;
    let list_request_id = send_link_context_packet(
        &transport,
        &link,
        PacketContext::Request,
        list_payload.as_slice(),
    )
    .await?
    .ok_or_else(|| std::io::Error::other("missing propagation request id"))?;

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_REQUEST_SENT;
        guard.state_name = "request_sent".to_string();
        guard.sync_progress = 0.5;
    });

    let list_response = wait_for_link_request_response(
        &mut data_rx,
        &mut resource_rx,
        relay_destination.desc.address_hash,
        link_id,
        list_request_id,
        PROPAGATION_REQUEST_TIMEOUT,
    )
    .await
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))?;
    if let Some((state_code, state_name, message, error_code)) =
        propagation_error_from_response_value(&list_response)
    {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = state_code;
            guard.state_name = state_name.to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some(message.to_string());
        });
        return Ok(json!({
            "success": false,
            "error": message,
            "errorCode": error_code,
            "state": state_code,
            "state_name": state_name,
            "progress": 0.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
        }));
    }

    let available_transient_ids = parse_binary_array(&list_response).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid propagation list response payload",
        )
    })?;

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_RESPONSE_RECEIVED;
        guard.state_name = "response_received".to_string();
        guard.sync_progress = 0.65;
    });

    let wants = available_transient_ids.into_iter().take(max_messages_usize).collect::<Vec<_>>();
    if wants.is_empty() {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = PR_COMPLETE;
            guard.state_name = "complete".to_string();
            guard.sync_progress = 1.0;
            guard.messages_received = 0;
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = None;
        });
        return Ok(json!({
            "success": true,
            "state": PR_COMPLETE,
            "state_name": "complete",
            "progress": 1.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
            "last_sync_started": started_at,
            "last_sync_completed": completed,
            "messages": [],
        }));
    }

    let get_payload = build_link_request_payload(
        "/get",
        rmpv::Value::Array(vec![
            rmpv::Value::Array(wants.iter().cloned().map(rmpv::Value::Binary).collect::<Vec<_>>()),
            rmpv::Value::Array(Vec::new()),
            rmpv::Value::Nil,
        ]),
    )?;
    let get_request_id =
        send_link_context_packet(&transport, &link, PacketContext::Request, get_payload.as_slice())
            .await?
            .ok_or_else(|| std::io::Error::other("missing propagation get request id"))?;

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_REQUEST_SENT;
        guard.state_name = "request_sent".to_string();
        guard.sync_progress = 0.75;
    });

    let get_response = wait_for_link_request_response(
        &mut data_rx,
        &mut resource_rx,
        relay_destination.desc.address_hash,
        link_id,
        get_request_id,
        PROPAGATION_REQUEST_TIMEOUT,
    )
    .await
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))?;
    if let Some((state_code, state_name, message, error_code)) =
        propagation_error_from_response_value(&get_response)
    {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = state_code;
            guard.state_name = state_name.to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some(message.to_string());
        });
        return Ok(json!({
            "success": false,
            "error": message,
            "errorCode": error_code,
            "state": state_code,
            "state_name": state_name,
            "progress": 0.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
        }));
    }

    let propagation_messages = parse_binary_array(&get_response).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid propagation message response payload",
        )
    })?;

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_RECEIVING;
        guard.state_name = "receiving".to_string();
        guard.sync_progress = 0.85;
    });

    let mut haves = Vec::new();
    let mut ingested_messages = Vec::new();
    for payload in &propagation_messages {
        let transient_id = Hash::new_from_slice(payload.as_slice()).to_bytes().to_vec();
        haves.push(transient_id.clone());

        if payload.len() >= 16 {
            let mut fallback_destination = [0u8; 16];
            fallback_destination.copy_from_slice(&payload[..16]);
            if let Some(record) = decode_inbound_payload(
                fallback_destination,
                payload.as_slice(),
                InboundPayloadMode::FullWire,
            ) {
                state.daemon.accept_inbound(record)?;
                ingested_messages.push(hex::encode(transient_id));
                continue;
            }
        }

        let _ = state.daemon.handle_rpc(RpcRequest {
            id: 0,
            method: "propagation_ingest".to_string(),
            params: Some(json!({
                "transient_id": hex::encode(transient_id),
                "payload_hex": hex::encode(payload),
            })),
        });
    }

    if !haves.is_empty() {
        if let Ok(sync_payload) = build_link_request_payload(
            "/get",
            rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(
                    haves.iter().cloned().map(rmpv::Value::Binary).collect::<Vec<_>>(),
                ),
            ]),
        ) {
            let _ = send_link_context_packet(
                &transport,
                &link,
                PacketContext::Request,
                sync_payload.as_slice(),
            )
            .await;
        }
    }

    let completed = now_epoch_secs() as i64;
    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_COMPLETE;
        guard.state_name = "complete".to_string();
        guard.sync_progress = 1.0;
        guard.messages_received = u32::try_from(propagation_messages.len()).unwrap_or(u32::MAX);
        guard.last_sync_started = Some(started_at);
        guard.last_sync_completed = Some(completed);
        guard.last_sync_error = None;
    });

    Ok(json!({
        "success": true,
        "state": PR_COMPLETE,
        "state_name": "complete",
        "progress": 1.0_f64,
        "messages_received": propagation_messages.len(),
        "selected_node": selected_node,
        "max_messages": max_messages,
        "last_sync_started": started_at,
        "last_sync_completed": completed,
        "messages": ingested_messages,
    }))
}
