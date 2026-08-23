impl RpcDaemon {
    pub fn is_blackholed(&self, identity_hash: &str) -> Result<bool, std::io::Error> {
        let identity_hash = normalize_blackhole_identity_hash(identity_hash).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "identity hash must be 16 bytes encoded as hexadecimal",
            )
        })?;
        self.prune_expired_blackholes()?;
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
                let identities = self.blackholed_identities_json()?;
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
                let until = parse_blackhole_until(parsed.until.as_ref())?;
                let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
                self.prune_expired_blackholes()?;
                let exists = self
                    .blackholed_identities
                    .lock()
                    .expect("blackholed_identities mutex poisoned")
                    .contains_key(identity_hash.as_str());
                if exists {
                    return Ok(RpcResponse {
                        id: request.id,
                        result: Some(JsonValue::Null),
                        error: None,
                    });
                }
                let bridge = self
                    .path_lookup_bridge
                    .lock()
                    .expect("path_lookup_bridge mutex poisoned")
                    .clone();
                if let Some(bridge) = bridge.as_ref() {
                    bridge.set_identity_blackholed_until(
                        identity_hash.as_str(),
                        true,
                        until,
                    )?;
                }
                {
                    let mut guard = self
                        .blackholed_identities
                        .lock()
                        .expect("blackholed_identities mutex poisoned");
                    guard.insert(
                        identity_hash.clone(),
                        json!({
                            "source": self.identity_hash.as_str(),
                            "until": parsed.until.unwrap_or(JsonValue::Null),
                            "reason": parsed.reason.unwrap_or(JsonValue::Null),
                        }),
                    );
                }
                if let Err(err) = self.persist_sdk_domain_snapshot() {
                    self.blackholed_identities
                        .lock()
                        .expect("blackholed_identities mutex poisoned")
                        .remove(identity_hash.as_str());
                    if let Some(bridge) = bridge {
                        if let Err(rollback_err) = bridge.set_identity_blackholed(
                            identity_hash.as_str(),
                            false,
                        ) {
                            log::error!(
                                "[daemon] failed to roll back transport blackhole {}: {}",
                                identity_hash,
                                rollback_err
                            );
                        }
                    }
                    return Err(err);
                }
                Ok(RpcResponse { id: request.id, result: Some(json!(true)), error: None })
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
                self.prune_expired_blackholes()?;
                let exists = self
                    .blackholed_identities
                    .lock()
                    .expect("blackholed_identities mutex poisoned")
                    .contains_key(identity_hash.as_str());
                if !exists {
                    return Ok(RpcResponse {
                        id: request.id,
                        result: Some(JsonValue::Null),
                        error: None,
                    });
                }
                if let Some(bridge) = self
                    .path_lookup_bridge
                    .lock()
                    .expect("path_lookup_bridge mutex poisoned")
                    .clone()
                {
                    bridge.set_identity_blackholed(identity_hash.as_str(), false)?;
                }
                {
                    let mut guard = self
                        .blackholed_identities
                        .lock()
                        .expect("blackholed_identities mutex poisoned");
                    guard.remove(identity_hash.as_str());
                }
                self.persist_sdk_domain_snapshot()?;
                Ok(RpcResponse { id: request.id, result: Some(json!(true)), error: None })
            }
            _ => unreachable!("legacy blackhole identity route: {}", request.method),
        }
    }

    fn blackholed_identities_json(&self) -> Result<JsonValue, std::io::Error> {
        self.prune_expired_blackholes()?;
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
        Ok(JsonValue::Object(identities))
    }

    fn prune_expired_blackholes(&self) -> Result<(), std::io::Error> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        let expired = self
            .blackholed_identities
            .lock()
            .expect("blackholed_identities mutex poisoned")
            .iter()
            .filter(|(_, entry)| {
                entry
                    .get("until")
                    .and_then(JsonValue::as_f64)
                    .is_some_and(|until| now > until)
            })
            .map(|(identity, _)| identity.clone())
            .collect::<Vec<_>>();
        if expired.is_empty() {
            return Ok(());
        }
        let bridge = self
            .path_lookup_bridge
            .lock()
            .expect("path_lookup_bridge mutex poisoned")
            .clone();
        if let Some(bridge) = bridge {
            for identity in &expired {
                bridge.set_identity_blackholed(identity.as_str(), false)?;
            }
        }
        let mut guard = self
            .blackholed_identities
            .lock()
            .expect("blackholed_identities mutex poisoned");
        for identity in expired {
            guard.remove(identity.as_str());
        }
        drop(guard);
        self.persist_sdk_domain_snapshot()
    }
}

fn parse_blackhole_until(until: Option<&JsonValue>) -> Result<Option<f64>, std::io::Error> {
    match until {
        None | Some(JsonValue::Null) => Ok(None),
        Some(value) => value.as_f64().filter(|value| value.is_finite()).map(Some).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "blackhole expiry must be a finite Unix timestamp",
            )
        }),
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
