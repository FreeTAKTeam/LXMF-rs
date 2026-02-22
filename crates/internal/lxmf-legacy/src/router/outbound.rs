use super::*;

impl Router {
    pub fn enqueue_outbound(&mut self, msg: WireMessage) {
        let message_id = msg.message_id().to_vec();
        let destination = msg.destination;
        let is_new = self.outbound_messages.insert(message_id.clone(), msg).is_none();
        self.outbound_progress.entry(message_id.clone()).or_insert(0);

        if is_new {
            if self.prioritised_destinations.contains(&destination) {
                self.outbound_queue.push_front(message_id);
            } else {
                self.outbound_queue.push_back(message_id);
            }
            self.stats.outbound_enqueued_total += 1;
        }
    }

    pub fn outbound_len(&self) -> usize {
        self.outbound_messages.len()
    }

    pub fn dequeue_outbound(&mut self) -> Option<WireMessage> {
        while let Some(message_id) = self.outbound_queue.pop_front() {
            if let Some(msg) = self.outbound_messages.remove(&message_id) {
                self.outbound_progress.remove(&message_id);
                return Some(msg);
            }
        }

        None
    }

    pub fn handle_outbound(
        &mut self,
        max_items: usize,
    ) -> Result<Vec<OutboundProcessResult>, LxmfError> {
        let mut results = Vec::new();
        let items_to_process = max_items.min(self.outbound_queue.len());

        for _ in 0..items_to_process {
            let Some(message_id) = self.outbound_queue.pop_front() else {
                break;
            };
            let Some(msg) = self.outbound_messages.remove(&message_id) else {
                self.outbound_progress.remove(&message_id);
                continue;
            };

            let destination = msg.destination;
            let status = if self.is_destination_ignored(&destination) {
                self.stats.outbound_ignored_total += 1;
                OutboundStatus::Ignored
            } else if !self.is_destination_allowed(&destination) {
                self.stats.outbound_rejected_auth_total += 1;
                OutboundStatus::RejectedAuth
            } else if let Some(transport_plugin) = self.transport_plugin.as_ref() {
                if !transport_plugin.has_outbound_sender() {
                    self.outbound_messages.insert(message_id.clone(), msg);
                    self.outbound_queue.push_back(message_id.clone());
                    OutboundStatus::DeferredNoAdapter
                } else {
                    let send_result = transport_plugin.send_outbound(&msg);
                    if let Err(_error) = send_result {
                        self.outbound_messages.insert(message_id.clone(), msg);
                        self.outbound_queue.push_back(message_id.clone());
                        self.stats.outbound_adapter_errors_total += 1;
                        OutboundStatus::DeferredAdapterError
                    } else {
                        for callback in &mut self.delivery_callbacks {
                            callback(&msg);
                        }
                        self.outbound_progress.insert(message_id.clone(), 100);
                        for callback in &mut self.outbound_progress_callbacks {
                            callback(&message_id, 100);
                        }
                        self.stats.outbound_processed_total += 1;
                        OutboundStatus::Sent
                    }
                }
            } else {
                self.outbound_messages.insert(message_id.clone(), msg);
                self.outbound_queue.push_back(message_id.clone());
                OutboundStatus::DeferredNoAdapter
            };

            results.push(OutboundProcessResult { message_id, destination, status });
        }

        Ok(results)
    }

    pub fn cancel_outbound(&mut self, message_id: &[u8]) -> bool {
        let removed_message = self.outbound_messages.remove(message_id);
        let removed_progress = self.outbound_progress.remove(message_id);
        let mut removed_from_queue = false;
        self.outbound_queue.retain(|id| {
            let keep = id.as_slice() != message_id;
            if !keep {
                removed_from_queue = true;
            }
            keep
        });

        let cancelled =
            removed_message.is_some() || removed_progress.is_some() || removed_from_queue;
        if cancelled {
            self.stats.outbound_cancelled_total += 1;
        }
        cancelled
    }

    pub fn set_outbound_progress(&mut self, message_id: &[u8], progress: u8) -> bool {
        let clamped = progress.min(100);
        match self.outbound_progress.get_mut(message_id) {
            Some(current) => {
                *current = clamped;
                for callback in &mut self.outbound_progress_callbacks {
                    callback(message_id, clamped);
                }
                true
            }
            None => false,
        }
    }

    pub fn outbound_progress(&self, message_id: &[u8]) -> Option<u8> {
        self.outbound_progress.get(message_id).copied()
    }

    pub fn cache_stamp(&mut self, material: &[u8], stamp: &[u8]) {
        self.stamp_cache.insert(material.to_vec(), stamp.to_vec());
    }

    pub fn cached_stamp(&self, material: &[u8]) -> Option<&[u8]> {
        self.stamp_cache.get(material).map(|v| v.as_slice())
    }

    pub fn remove_cached_stamp(&mut self, material: &[u8]) -> Option<Vec<u8>> {
        self.stamp_cache.remove(material)
    }

    pub fn cache_ticket(&mut self, destination: [u8; 16], ticket: Ticket) {
        self.ticket_cache.insert(destination, ticket);
    }

    pub fn ticket_for(&self, destination: &[u8; 16]) -> Option<&Ticket> {
        self.ticket_cache.get(destination)
    }

    pub fn remove_ticket(&mut self, destination: &[u8; 16]) -> Option<Ticket> {
        self.ticket_cache.remove(destination)
    }
}
