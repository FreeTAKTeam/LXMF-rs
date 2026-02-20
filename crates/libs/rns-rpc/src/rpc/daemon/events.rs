impl RpcDaemon {
    pub fn handle_framed_request(&self, bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
        let request: RpcRequest = codec::decode_frame(bytes)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let response = self.handle_rpc(request)?;
        codec::encode_frame(&response).map_err(std::io::Error::other)
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<RpcEvent> {
        self.events.subscribe()
    }

    pub fn take_event(&self) -> Option<RpcEvent> {
        let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
        guard.pop_front()
    }

    pub fn push_event(&self, event: RpcEvent) {
        {
            let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
            if guard.len() >= 32 {
                guard.pop_front();
            }
            guard.push_back(event.clone());
        }

        let seq_no = {
            let mut seq_guard =
                self.sdk_next_event_seq.lock().expect("sdk_next_event_seq mutex poisoned");
            *seq_guard = seq_guard.saturating_add(1);
            *seq_guard
        };
        let mut log_guard = self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
        if log_guard.len() >= SDK_EVENT_LOG_CAPACITY {
            log_guard.pop_front();
            let mut dropped = self
                .sdk_dropped_event_count
                .lock()
                .expect("sdk_dropped_event_count mutex poisoned");
            *dropped = dropped.saturating_add(1);
        }
        log_guard.push_back(SequencedRpcEvent { seq_no, event });
    }

    pub fn emit_event(&self, event: RpcEvent) {
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }

    pub fn schedule_announce_for_test(&self, id: u64) {
        let timestamp = now_i64();
        let event = RpcEvent {
            event_type: "announce_sent".into(),
            payload: json!({ "timestamp": timestamp, "announce_id": id }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }

    pub fn start_announce_scheduler(
        self: std::rc::Rc<Self>,
        interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn_local(async move {
            if interval_secs == 0 {
                return;
            }

            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            loop {
                // First tick is immediate, so we announce once at scheduler start.
                interval.tick().await;
                let id = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_secs())
                    .unwrap_or(0);

                if let Some(bridge) = &self.announce_bridge {
                    let _ = bridge.announce_now();
                }

                let timestamp = now_i64();
                let event = RpcEvent {
                    event_type: "announce_sent".into(),
                    payload: json!({ "timestamp": timestamp, "announce_id": id }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);
            }
        })
    }

    pub fn inject_inbound_test_message(&self, content: &str) {
        let timestamp = now_i64();
        let record = crate::storage::messages::MessageRecord {
            id: format!("test-{}", timestamp),
            source: "test-peer".into(),
            destination: "local".into(),
            title: "".into(),
            content: content.into(),
            timestamp,
            direction: "in".into(),
            fields: None,
            receipt_status: None,
        };
        let _ = self.store.insert_message(&record);
        let event =
            RpcEvent { event_type: "inbound".into(), payload: json!({ "message": record }) };
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }

    pub fn emit_link_event_for_test(&self) {
        let event = RpcEvent {
            event_type: "link_activated".into(),
            payload: json!({ "link_id": "test-link" }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }
}
