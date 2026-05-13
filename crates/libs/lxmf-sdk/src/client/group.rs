use super::*;

impl<B: SdkBackend> LxmfSdkGroupDelivery for Client<B> {
    fn send_group(&self, req: GroupSendRequest) -> Result<GroupSendResult, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Send)?;
        }

        let source = req.source.trim();
        if source.is_empty() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "group send source must not be empty",
            )
            .with_user_actionable(true));
        }
        if req.destinations.is_empty() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "group send requires at least one destination",
            )
            .with_user_actionable(true));
        }

        let mut outcomes = Vec::with_capacity(req.destinations.len());
        for destination in req.destinations {
            let trimmed_destination = destination.trim().to_owned();
            if trimmed_destination.is_empty() {
                outcomes.push(GroupSendOutcome {
                    destination,
                    state: GroupRecipientState::Failed,
                    message_id: None,
                    retryable: false,
                    reason_code: Some(code::VALIDATION_INVALID_ARGUMENT.to_owned()),
                });
                continue;
            }

            let send_request = SendRequest {
                source: source.to_owned(),
                destination: trimmed_destination.clone(),
                payload: req.payload.clone(),
                idempotency_key: req.idempotency_key.clone(),
                ttl_ms: req.ttl_ms,
                correlation_id: req.correlation_id.clone(),
                extensions: req.extensions.clone(),
            };
            match self.send(send_request) {
                Ok(message_id) => outcomes.push(GroupSendOutcome {
                    destination: trimmed_destination,
                    state: GroupRecipientState::Accepted,
                    message_id: Some(message_id),
                    retryable: false,
                    reason_code: None,
                }),
                Err(err) => {
                    let state = if err.is_retryable() {
                        GroupRecipientState::Deferred
                    } else {
                        GroupRecipientState::Failed
                    };
                    outcomes.push(GroupSendOutcome {
                        destination: trimmed_destination,
                        state,
                        message_id: None,
                        retryable: err.is_retryable(),
                        reason_code: Some(err.machine_code),
                    });
                }
            }
        }

        let accepted_count = outcomes
            .iter()
            .filter(|outcome| outcome.state == GroupRecipientState::Accepted)
            .count();
        let deferred_count = outcomes
            .iter()
            .filter(|outcome| outcome.state == GroupRecipientState::Deferred)
            .count();
        let failed_count =
            outcomes.iter().filter(|outcome| outcome.state == GroupRecipientState::Failed).count();

        Ok(GroupSendResult { outcomes, accepted_count, deferred_count, failed_count })
    }
}
