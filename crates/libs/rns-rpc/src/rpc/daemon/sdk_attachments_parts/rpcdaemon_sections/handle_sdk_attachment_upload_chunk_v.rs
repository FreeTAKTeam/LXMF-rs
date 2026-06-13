impl RpcDaemon {

    pub(super) fn handle_sdk_attachment_upload_chunk_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.attachment_streaming") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_attachment_upload_chunk_v2",
                "sdk.capability.attachment_streaming",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkAttachmentUploadChunkV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let upload_id = match Self::normalize_non_empty(parsed.upload_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "upload_id must not be empty",
                ))
            }
        };
        let decoded_bytes =
            BASE64_STANDARD.decode(parsed.bytes_base64.as_bytes()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "attachment chunk bytes_base64 is invalid",
                )
            })?;
        if decoded_bytes.is_empty() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "attachment upload chunk must not be empty",
            ));
        }

        let mut uploads =
            self.sdk_attachment_uploads.lock().expect("sdk_attachment_uploads mutex poisoned");
        let Some(upload) = uploads.get_mut(upload_id.as_str()) else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "attachment upload session not found",
            ));
        };
        if parsed.offset != upload.next_offset {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_INVALID_CURSOR",
                "attachment upload offset does not match next_offset",
            ));
        }
        let next_offset = upload.next_offset.saturating_add(decoded_bytes.len() as u64);
        if next_offset > upload.total_size {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "attachment upload exceeds declared total_size",
            ));
        }
        upload.payload.extend_from_slice(decoded_bytes.as_slice());
        upload.next_offset = next_offset;

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "upload_chunk": {
                    "accepted": true,
                    "next_offset": next_offset,
                    "complete": next_offset == upload.total_size,
                }
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_attachment_upload_commit_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.attachment_streaming") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_attachment_upload_commit_v2",
                "sdk.capability.attachment_streaming",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkAttachmentUploadCommitV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let upload_id = match Self::normalize_non_empty(parsed.upload_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "upload_id must not be empty",
                ))
            }
        };
        let upload = {
            let mut uploads =
                self.sdk_attachment_uploads.lock().expect("sdk_attachment_uploads mutex poisoned");
            uploads.remove(upload_id.as_str())
        };
        let Some(upload) = upload else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "attachment upload session not found",
            ));
        };
        if upload.next_offset != upload.total_size {
            self.sdk_attachment_uploads
                .lock()
                .expect("sdk_attachment_uploads mutex poisoned")
                .insert(upload.upload_id.clone(), upload);
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "attachment upload is incomplete",
            ));
        }

        let mut hasher = Sha256::new();
        hasher.update(upload.payload.as_slice());
        let checksum = encode_hex(hasher.finalize());
        if checksum != upload.checksum_sha256 {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_CHECKSUM_MISMATCH",
                "attachment checksum does not match committed bytes",
            ));
        }
        let bytes_base64 = BASE64_STANDARD.encode(upload.payload.as_slice());
        let record = SdkAttachmentRecord {
            attachment_id: upload.attachment_id.clone(),
            name: upload.name,
            content_type: upload.content_type,
            byte_len: upload.total_size,
            checksum_sha256: checksum,
            created_ts_ms: now_millis_u64(),
            expires_ts_ms: upload.expires_ts_ms,
            topic_ids: upload.topic_ids,
            extensions: upload.extensions,
        };
        self.sdk_attachments
            .lock()
            .expect("sdk_attachments mutex poisoned")
            .insert(upload.attachment_id.clone(), record.clone());
        self.sdk_attachment_payloads
            .lock()
            .expect("sdk_attachment_payloads mutex poisoned")
            .insert(upload.attachment_id.clone(), bytes_base64);
        self.sdk_attachment_order
            .lock()
            .expect("sdk_attachment_order mutex poisoned")
            .push(upload.attachment_id.clone());
        self.persist_sdk_domain_snapshot()?;
        self.publish_event(RpcEvent {
            event_type: "sdk_attachment_stored".to_string(),
            payload: json!({
                "attachment_id": upload.attachment_id,
                "byte_len": record.byte_len,
            }),
        });
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "attachment": record })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_attachment_download_chunk_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.attachment_streaming") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_attachment_download_chunk_v2",
                "sdk.capability.attachment_streaming",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkAttachmentDownloadChunkV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let attachment_id = match Self::normalize_non_empty(parsed.attachment_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "attachment_id must not be empty",
                ))
            }
        };
        let offset = parsed.offset.unwrap_or(0);
        let max_bytes = parsed.max_bytes.unwrap_or(65_536).clamp(1, 1_048_576);
        let payload = self
            .sdk_attachment_payloads
            .lock()
            .expect("sdk_attachment_payloads mutex poisoned")
            .get(attachment_id.as_str())
            .cloned();
        let Some(payload_base64) = payload else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "attachment not found",
            ));
        };
        let payload_bytes = BASE64_STANDARD.decode(payload_base64.as_bytes()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stored attachment payload is not valid base64",
            )
        })?;
        if offset > payload_bytes.len() as u64 {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_INVALID_CURSOR",
                "attachment download offset is out of range",
            ));
        }
        let start = offset as usize;
        let end = start.saturating_add(max_bytes).min(payload_bytes.len());
        let chunk = &payload_bytes[start..end];
        let next_offset = end as u64;
        let record = self
            .sdk_attachments
            .lock()
            .expect("sdk_attachments mutex poisoned")
            .get(attachment_id.as_str())
            .cloned();
        let Some(record) = record else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "attachment metadata not found",
            ));
        };
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "download_chunk": {
                    "attachment_id": attachment_id,
                    "offset": offset,
                    "next_offset": next_offset,
                    "total_size": payload_bytes.len() as u64,
                    "done": next_offset >= payload_bytes.len() as u64,
                    "checksum_sha256": record.checksum_sha256,
                    "bytes_base64": BASE64_STANDARD.encode(chunk),
                }
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_attachment_associate_topic_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.attachments") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_attachment_associate_topic_v2",
                "sdk.capability.attachments",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkAttachmentAssociateTopicV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let attachment_id = match Self::normalize_non_empty(parsed.attachment_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "attachment_id must not be empty",
                ))
            }
        };
        let topic_id = match Self::normalize_non_empty(parsed.topic_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "topic_id must not be empty",
                ))
            }
        };
        if !self
            .sdk_topics
            .lock()
            .expect("sdk_topics mutex poisoned")
            .contains_key(topic_id.as_str())
        {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "topic not found",
            ));
        }
        {
            let mut attachments =
                self.sdk_attachments.lock().expect("sdk_attachments mutex poisoned");
            let Some(record) = attachments.get_mut(attachment_id.as_str()) else {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_NOT_FOUND",
                    "attachment not found",
                ));
            };
            if !record.topic_ids.iter().any(|current| current == topic_id.as_str()) {
                record.topic_ids.push(topic_id.clone());
            }
        }
        self.persist_sdk_domain_snapshot()?;
        Ok(RpcResponse {
            id: request.id,
            result: Some(
                json!({ "accepted": true, "attachment_id": attachment_id, "topic_id": topic_id }),
            ),
            error: None,
        })
    }
}
