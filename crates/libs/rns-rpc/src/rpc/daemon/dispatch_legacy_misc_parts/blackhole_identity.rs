impl RpcDaemon {
    pub fn is_blackholed(&self, identity_hash: &str) -> Result<bool, std::io::Error> {
        let identity_hash = normalize_blackhole_identity_hash(identity_hash).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "identity hash must be 16 bytes encoded as hexadecimal",
            )
        })?;
        let guard = self
            .blackholed_identities
            .lock()
            .expect("blackholed_identities mutex poisoned");
        Ok(guard.contains_key(identity_hash.as_str()))
    }

    fn handle_rpc_legacy_blackhole_identity(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "get_blackholed_identities" => {
                let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
                let identities = self.blackholed_identities_json();
                Ok(RpcResponse { id: request.id, result: Some(identities), error: None })
            }
            "blackhole_identity" => {
                let parsed = parse_blackhole_identity_params(request.params)?;
                let Some(identity_hash) = normalize_blackhole_identity_hash(&parsed.identity)
                else {
                    return Ok(RpcResponse {
                        id: request.id,
                        result: Some(json!(false)),
                        error: None,
                    });
                };
                let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
                let changed = {
                    let mut guard = self
                        .blackholed_identities
                        .lock()
                        .expect("blackholed_identities mutex poisoned");
                    if guard.contains_key(identity_hash.as_str()) {
                        JsonValue::Null
                    } else {
                        guard.insert(
                            identity_hash.clone(),
                            json!({
                                "source": self.identity_hash.as_str(),
                                "until": parsed.until.unwrap_or(JsonValue::Null),
                                "reason": parsed.reason.unwrap_or(JsonValue::Null),
                            }),
                        );
                        json!(true)
                    }
                };
                if changed == json!(true) {
                    self.persist_sdk_domain_snapshot()?;
                    if let Some(bridge) = self
                        .path_lookup_bridge
                        .lock()
                        .expect("path_lookup_bridge mutex poisoned")
                        .clone()
                    {
                        if let Err(err) = bridge.remove_paths_for_identity(identity_hash.as_str()) {
                            log::warn!(
                                "[daemon] failed to remove paths for blackholed identity {}: {}",
                                identity_hash,
                                err
                            );
                        }
                    }
                }
                Ok(RpcResponse { id: request.id, result: Some(changed), error: None })
            }
            "unblackhole_identity" => {
                let parsed = parse_blackhole_identity_params(request.params)?;
                let Some(identity_hash) = normalize_blackhole_identity_hash(&parsed.identity)
                else {
                    return Ok(RpcResponse {
                        id: request.id,
                        result: Some(json!(false)),
                        error: None,
                    });
                };
                let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
                let removed = {
                    let mut guard = self
                        .blackholed_identities
                        .lock()
                        .expect("blackholed_identities mutex poisoned");
                    if guard.remove(identity_hash.as_str()).is_some() {
                        json!(true)
                    } else {
                        JsonValue::Null
                    }
                };
                if removed == json!(true) {
                    self.persist_sdk_domain_snapshot()?;
                }
                Ok(RpcResponse { id: request.id, result: Some(removed), error: None })
            }
            _ => unreachable!("legacy blackhole identity route: {}", request.method),
        }
    }

    fn blackholed_identities_json(&self) -> JsonValue {
        let guard =
            self.blackholed_identities.lock().expect("blackholed_identities mutex poisoned");
        let mut identities = JsonMap::new();
        let mut keys = guard.keys().collect::<Vec<_>>();
        keys.sort();
        for key in keys {
            if let Some(value) = guard.get(key.as_str()) {
                identities.insert(key.clone(), value.clone());
            }
        }
        JsonValue::Object(identities)
    }
}

fn parse_blackhole_identity_params(
    params: Option<JsonValue>,
) -> Result<BlackholeIdentityParams, std::io::Error> {
    let params = params
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params"))?;
    serde_json::from_value(params)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))
}

fn normalize_blackhole_identity_hash(identity: &str) -> Option<String> {
    let identity = identity.trim();
    let Ok(decoded) = hex::decode(identity) else {
        return None;
    };
    (decoded.len() == 16).then(|| identity.to_ascii_lowercase())
}
