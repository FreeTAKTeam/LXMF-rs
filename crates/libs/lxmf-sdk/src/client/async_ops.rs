use super::*;

#[cfg(feature = "sdk-async")]
impl<B: SdkBackendAsyncOps> Client<B> {
    pub async fn start_async(&self, req: StartRequest) -> Result<ClientHandle, SdkError> {
        req.validate()?;

        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            if lifecycle.check_start_reentry(&req)? {
                return self
                    .handle
                    .lock()
                    .expect("client handle mutex poisoned")
                    .clone()
                    .ok_or_else(|| {
                        SdkError::new(
                            code::INTERNAL,
                            ErrorCategory::Internal,
                            "runtime is running but client handle is missing",
                        )
                    });
            }
        }

        {
            let mut lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.mark_starting()?;
        }

        let negotiation = match self
            .backend
            .negotiate_async(NegotiationRequest {
                supported_contract_versions: req.supported_contract_versions.clone(),
                requested_capabilities: req.requested_capabilities.clone(),
                profile: req.config.profile.clone(),
                bind_mode: req.config.bind_mode.clone(),
                auth_mode: req.config.auth_mode.clone(),
                overflow_policy: req.config.overflow_policy.clone(),
                block_timeout_ms: req.config.block_timeout_ms,
                rpc_backend: req.config.rpc_backend.clone(),
                extensions: req.config.extensions.clone(),
            })
            .await
        {
            Ok(negotiation) => negotiation,
            Err(err) => {
                self.rollback_start_transition();
                return Err(err);
            }
        };
        if let Err(err) = Self::ensure_capabilities(
            req.config.profile.clone(),
            &req.requested_capabilities,
            &negotiation,
        ) {
            self.rollback_start_transition();
            return Err(err);
        }
        let handle = Self::as_client_handle(negotiation);
        {
            let mut guard = self.handle.lock().expect("client handle mutex poisoned");
            *guard = Some(handle.clone());
        }

        {
            let mut lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            if let Err(err) = lifecycle.mark_running(req) {
                let mut guard = self.handle.lock().expect("client handle mutex poisoned");
                *guard = None;
                if lifecycle.state() == RuntimeState::Starting {
                    lifecycle.reset_to_new();
                }
                return Err(err);
            }
        }

        Ok(handle)
    }

    pub async fn send_async(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Send)?;
        }

        let Some(idempotency_key) = req.idempotency_key.clone() else {
            return self.backend.send_async(req).await;
        };

        let ttl_ms = self
            .current_limits()
            .ok()
            .flatten()
            .map(|limits| limits.idempotency_ttl_ms)
            .unwrap_or(86_400_000);
        let now = Instant::now();
        let cache_key = (req.source.clone(), req.destination.clone(), idempotency_key);
        let payload_hash = Self::payload_hash(&req.payload)?;

        let inflight_lock = {
            let mut inflight =
                self.idempotency_inflight.lock().expect("idempotency_inflight mutex poisoned");
            inflight
                .entry(cache_key.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let idempotency_guard = inflight_lock.lock().await;

        let cached_result = {
            let mut cache =
                self.idempotency_cache.lock().expect("idempotency_cache mutex poisoned");
            cache.retain(|_, record| {
                now.duration_since(record.seen_at).as_millis() <= u128::from(ttl_ms)
            });
            cache.get(&cache_key).map(|existing| {
                if existing.payload_hash == payload_hash {
                    Ok(existing.message_id.clone())
                } else {
                    Err(SdkError::new(
                        code::VALIDATION_IDEMPOTENCY_CONFLICT,
                        ErrorCategory::Validation,
                        "idempotency key already used for different payload",
                    )
                    .with_user_actionable(true))
                }
            })
        };
        if let Some(result) = cached_result {
            drop(idempotency_guard);
            let mut inflight =
                self.idempotency_inflight.lock().expect("idempotency_inflight mutex poisoned");
            if inflight.get(&cache_key).is_some_and(|current| Arc::ptr_eq(current, &inflight_lock))
            {
                inflight.remove(&cache_key);
            }
            return result;
        }

        let message_id = match self.backend.send_async(req).await {
            Ok(message_id) => message_id,
            Err(err) => {
                drop(idempotency_guard);
                let mut inflight =
                    self.idempotency_inflight.lock().expect("idempotency_inflight mutex poisoned");
                if inflight
                    .get(&cache_key)
                    .is_some_and(|current| Arc::ptr_eq(current, &inflight_lock))
                {
                    inflight.remove(&cache_key);
                }
                return Err(err);
            }
        };
        let mut cache = self.idempotency_cache.lock().expect("idempotency_cache mutex poisoned");
        cache.entry(cache_key.clone()).or_insert_with(|| IdempotencyRecord {
            payload_hash,
            message_id: message_id.clone(),
            seen_at: now,
        });
        drop(cache);
        drop(idempotency_guard);
        let mut inflight =
            self.idempotency_inflight.lock().expect("idempotency_inflight mutex poisoned");
        if inflight.get(&cache_key).is_some_and(|current| Arc::ptr_eq(current, &inflight_lock)) {
            inflight.remove(&cache_key);
        }
        Ok(message_id)
    }

    pub async fn status_async(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Status)?;
        }
        self.backend.status_async(id).await
    }

    pub async fn cancel_async(&self, id: MessageId) -> Result<CancelResult, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Cancel)?;
        }
        self.backend.cancel_async(id).await
    }

    pub async fn configure_async(
        &self,
        expected_revision: u64,
        patch: ConfigPatch,
    ) -> Result<Ack, SdkError> {
        if patch.is_empty() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "config patch must contain at least one key",
            )
            .with_user_actionable(true));
        }
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Configure)?;
        }
        self.backend.configure_async(expected_revision, patch).await
    }

    pub async fn poll_events_async(
        &self,
        cursor: Option<EventCursor>,
        max: usize,
    ) -> Result<EventBatch, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::PollEvents)?;
        }
        if max == 0 {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "poll max must be greater than zero",
            )
            .with_user_actionable(true));
        }
        if let Some(limits) = self.current_limits().ok().flatten() {
            if max > limits.max_poll_events {
                return Err(SdkError::new(
                    code::VALIDATION_MAX_POLL_EVENTS_EXCEEDED,
                    ErrorCategory::Validation,
                    "poll max exceeds negotiated effective_limits.max_poll_events",
                )
                .with_user_actionable(true));
            }
        }
        self.backend.poll_events_async(cursor, max).await
    }

    pub async fn snapshot_async(&self) -> Result<RuntimeSnapshot, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Snapshot)?;
        }
        self.backend.snapshot_async().await
    }

    pub async fn shutdown_async(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
        let current_state = {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Shutdown)?;
            lifecycle.state()
        };
        if current_state == RuntimeState::Stopped {
            return Ok(Ack { accepted: true, revision: None });
        }
        let ack = self.backend.shutdown_async(mode).await?;
        {
            let mut lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            if matches!(lifecycle.state(), RuntimeState::Running | RuntimeState::Starting) {
                lifecycle.mark_draining()?;
            }
            lifecycle.mark_stopped();
        }
        Ok(ack)
    }
}
