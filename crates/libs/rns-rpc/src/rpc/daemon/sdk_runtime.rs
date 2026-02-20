impl RpcDaemon {
    fn handle_sdk_cancel_message_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkCancelMessageV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let message_id = parsed.message_id.trim();
        if message_id.is_empty() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "message_id must not be empty",
            ));
        }

        let _status_guard =
            self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
        let message = self.store.get_message(message_id).map_err(std::io::Error::other)?;
        if message.is_none() {
            return Ok(RpcResponse {
                id: request.id,
                result: Some(json!({
                    "message_id": message_id,
                    "result": "NotFound",
                })),
                error: None,
            });
        }

        let message_status = message.and_then(|record| record.receipt_status);

        let transitions = self
            .delivery_traces
            .lock()
            .expect("delivery traces mutex poisoned")
            .get(message_id)
            .cloned()
            .unwrap_or_default();

        let mut cancel_result = "Accepted";
        if let Some(status) = &message_status {
            let normalized = status.trim().to_ascii_lowercase();
            if normalized.starts_with("sent") {
                cancel_result = "TooLateToCancel";
            } else if matches!(
                normalized.as_str(),
                "cancelled" | "delivered" | "failed" | "expired" | "rejected"
            ) {
                cancel_result = "AlreadyTerminal";
            }
        }

        for transition in &transitions {
            if cancel_result != "Accepted" {
                break;
            }
            let normalized = transition.status.trim().to_ascii_lowercase();
            if normalized.starts_with("sent") {
                cancel_result = "TooLateToCancel";
                break;
            }
            if matches!(
                normalized.as_str(),
                "cancelled" | "delivered" | "failed" | "expired" | "rejected"
            ) {
                cancel_result = "AlreadyTerminal";
                break;
            }
        }

        if cancel_result == "Accepted" {
            self.store
                .update_receipt_status(message_id, "cancelled")
                .map_err(std::io::Error::other)?;
            self.append_delivery_trace(message_id, "cancelled".to_string());
            let event = RpcEvent {
                event_type: "delivery_cancelled".into(),
                payload: json!({ "message_id": message_id, "result": "Accepted" }),
            };
            self.publish_event(event);
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "message_id": message_id,
                "result": cancel_result,
            })),
            error: None,
        })
    }

    fn handle_sdk_status_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkStatusV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let message_id = parsed.message_id.trim();
        if message_id.is_empty() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "message_id must not be empty",
            ));
        }
        let message = self.store.get_message(message_id).map_err(std::io::Error::other)?;
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "message": message,
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }

    fn handle_sdk_configure_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkConfigureV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

        let patch_map = parsed.patch.as_object().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "patch must be an object")
        })?;
        const ALLOWED_KEYS: &[&str] = &[
            "overflow_policy",
            "block_timeout_ms",
            "event_stream",
            "idempotency_ttl_ms",
            "redaction",
            "rpc_backend",
            "extensions",
        ];
        if let Some(key) = patch_map.keys().find(|key| !ALLOWED_KEYS.contains(&key.as_str())) {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_CONFIG_UNKNOWN_KEY",
                &format!("unknown config key '{key}'"),
            ));
        }

        let _apply_guard =
            self.sdk_config_apply_lock.lock().expect("sdk_config_apply_lock mutex poisoned");
        let mut revision_guard =
            self.sdk_config_revision.lock().expect("sdk_config_revision mutex poisoned");
        if parsed.expected_revision != *revision_guard {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_CONFIG_CONFLICT",
                "config revision mismatch",
            ));
        }
        *revision_guard = revision_guard.saturating_add(1);
        let revision = *revision_guard;

        {
            let mut config_guard =
                self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned");
            merge_json_patch(&mut config_guard, &parsed.patch);
        }
        drop(revision_guard);

        let event = RpcEvent {
            event_type: "config_updated".into(),
            payload: json!({
                "revision": revision,
                "patch": parsed.patch,
            }),
        };
        self.publish_event(event);

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "revision": revision,
            })),
            error: None,
        })
    }

    fn handle_sdk_shutdown_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkShutdownV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let mode = parsed.mode.trim().to_ascii_lowercase();
        if mode != "graceful" && mode != "immediate" {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "shutdown mode must be 'graceful' or 'immediate'",
            ));
        }

        let event = RpcEvent {
            event_type: "runtime_shutdown_requested".into(),
            payload: json!({
                "mode": mode,
                "flush_timeout_ms": parsed.flush_timeout_ms,
            }),
        };
        self.publish_event(event);

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "mode": mode,
            })),
            error: None,
        })
    }

    fn handle_sdk_snapshot_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request
            .params
            .map(serde_json::from_value::<SdkSnapshotV2Params>)
            .transpose()
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
            .unwrap_or_default();
        let active_contract_version = self.active_contract_version();
        let event_stream_position = self
            .sdk_event_log
            .lock()
            .expect("sdk_event_log mutex poisoned")
            .back()
            .map(|entry| entry.seq_no)
            .unwrap_or(0);
        let config_revision =
            *self.sdk_config_revision.lock().expect("sdk_config_revision mutex poisoned");
        let profile = self.sdk_profile.lock().expect("sdk_profile mutex poisoned").clone();
        let effective_capabilities = self
            .sdk_effective_capabilities
            .lock()
            .expect("sdk_effective_capabilities mutex poisoned")
            .clone();

        let (queued_messages, in_flight_messages) =
            self.store.count_message_buckets().map_err(std::io::Error::other)?;

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "runtime_id": self.identity_hash,
                "state": "running",
                "active_contract_version": active_contract_version,
                "event_stream_position": event_stream_position,
                "config_revision": config_revision,
                "profile": profile,
                "effective_capabilities": effective_capabilities,
                "queued_messages": queued_messages,
                "in_flight_messages": in_flight_messages,
                "counts_included": params.include_counts,
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }

}
