impl RpcDaemon {
    fn handle_rpc_legacy_clear(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "clear_messages" => {
                self.store.clear_messages().map_err(std::io::Error::other)?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "cleared": "messages" })),
                    error: None,
                })
            }
            "clear_resources" => {
                let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
                self.sdk_attachments.lock().expect("sdk_attachments mutex poisoned").clear();
                self.sdk_attachment_payloads
                    .lock()
                    .expect("sdk_attachment_payloads mutex poisoned")
                    .clear();
                self.sdk_attachment_order
                    .lock()
                    .expect("sdk_attachment_order mutex poisoned")
                    .clear();
                self.sdk_attachment_uploads
                    .lock()
                    .expect("sdk_attachment_uploads mutex poisoned")
                    .clear();
                self.sdk_topics.lock().expect("sdk_topics mutex poisoned").clear();
                self.sdk_topic_order.lock().expect("sdk_topic_order mutex poisoned").clear();
                self.sdk_topic_subscriptions
                    .lock()
                    .expect("sdk_topic_subscriptions mutex poisoned")
                    .clear();
                self.sdk_markers.lock().expect("sdk_markers mutex poisoned").clear();
                self.sdk_marker_order.lock().expect("sdk_marker_order mutex poisoned").clear();
                self.persist_sdk_domain_snapshot()?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "cleared": "resources" })),
                    error: None,
                })
            }
            "clear_peers" => {
                {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    guard.clear();
                }
                self.store.clear_announces().map_err(std::io::Error::other)?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "cleared": "peers" })),
                    error: None,
                })
            }
            "clear_all" => {
                let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
                self.store.clear_messages().map_err(std::io::Error::other)?;
                self.store.clear_announces().map_err(std::io::Error::other)?;
                {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    guard.clear();
                }
                {
                    let mut guard =
                        self.delivery_traces.lock().expect("delivery traces mutex poisoned");
                    guard.clear();
                }
                self.sdk_attachments.lock().expect("sdk_attachments mutex poisoned").clear();
                self.sdk_attachment_payloads
                    .lock()
                    .expect("sdk_attachment_payloads mutex poisoned")
                    .clear();
                self.sdk_attachment_order
                    .lock()
                    .expect("sdk_attachment_order mutex poisoned")
                    .clear();
                self.sdk_attachment_uploads
                    .lock()
                    .expect("sdk_attachment_uploads mutex poisoned")
                    .clear();
                self.sdk_topics.lock().expect("sdk_topics mutex poisoned").clear();
                self.sdk_topic_order.lock().expect("sdk_topic_order mutex poisoned").clear();
                self.sdk_topic_subscriptions
                    .lock()
                    .expect("sdk_topic_subscriptions mutex poisoned")
                    .clear();
                self.sdk_markers.lock().expect("sdk_markers mutex poisoned").clear();
                self.sdk_marker_order.lock().expect("sdk_marker_order mutex poisoned").clear();
                self.sdk_telemetry_points
                    .lock()
                    .expect("sdk_telemetry_points mutex poisoned")
                    .clear();
                self.sdk_remote_commands
                    .lock()
                    .expect("sdk_remote_commands mutex poisoned")
                    .clear();
                self.sdk_voice_sessions.lock().expect("sdk_voice_sessions mutex poisoned").clear();
                self.persist_sdk_domain_snapshot()?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "cleared": "all" })),
                    error: None,
                })
            }
            _ => unreachable!("legacy clear route: {}", request.method),
        }
    }
}
