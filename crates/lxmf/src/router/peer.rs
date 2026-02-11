use super::*;

impl Router {
    pub fn register_peer(&mut self, destination: [u8; 16]) -> bool {
        match self.peers.entry(destination) {
            Entry::Vacant(entry) => {
                entry.insert(Peer::new(destination));
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    pub fn remove_peer(&mut self, destination: &[u8; 16]) -> Option<Peer> {
        self.peers.remove(destination)
    }

    pub fn peer(&self, destination: &[u8; 16]) -> Option<&Peer> {
        self.peers.get(destination)
    }

    pub fn peer_mut(&mut self, destination: &[u8; 16]) -> Option<&mut Peer> {
        self.peers.get_mut(destination)
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn queue_peer_unhandled(&mut self, destination: [u8; 16], transient_id: &[u8]) {
        let peer = self.peers.entry(destination).or_insert_with(|| Peer::new(destination));
        peer.queue_unhandled_message(transient_id);
    }

    pub fn queue_peer_handled(&mut self, destination: [u8; 16], transient_id: &[u8]) {
        let peer = self.peers.entry(destination).or_insert_with(|| Peer::new(destination));
        peer.queue_handled_message(transient_id);
    }

    pub fn process_peer_queues(&mut self, destination: &[u8; 16]) -> bool {
        let Some(peer) = self.peers.get_mut(destination) else {
            return false;
        };
        peer.process_queues();
        true
    }

    pub fn ingest_lxm_uri(&mut self, uri: &str) -> Result<PaperIngestResult, LxmfError> {
        let paper = WireMessage::decode_lxm_uri(uri)?;
        self.ingest_paper_message_bytes(&paper)
    }

    pub fn ingest_paper_message_bytes(
        &mut self,
        paper: &[u8],
    ) -> Result<PaperIngestResult, LxmfError> {
        if paper.len() <= 16 {
            return Err(LxmfError::Decode("paper message too short".into()));
        }

        let mut destination = [0u8; 16];
        destination.copy_from_slice(&paper[..16]);
        let transient_id = reticulum::hash::Hash::new_from_slice(paper).to_bytes().to_vec();
        let duplicate = self.paper_messages.contains_key(&transient_id);

        if duplicate {
            self.stats.paper_uri_duplicate_total += 1;
        } else {
            self.paper_messages.insert(transient_id.clone(), paper.to_vec());
            self.stats.paper_uri_ingested_total += 1;
            self.register_peer(destination);
            self.queue_peer_unhandled(destination, &transient_id);
        }

        Ok(PaperIngestResult { destination, transient_id, bytes_len: paper.len(), duplicate })
    }

    pub fn paper_message(&self, transient_id: &[u8]) -> Option<&[u8]> {
        self.paper_messages.get(transient_id).map(std::vec::Vec::as_slice)
    }

    pub fn paper_message_count(&self) -> usize {
        self.paper_messages.len()
    }

    pub fn build_peer_sync_batch(
        &mut self,
        destination: &[u8; 16],
        requested: usize,
    ) -> Vec<Vec<u8>> {
        let max_items = requested.min(self.config.propagation_per_sync_limit as usize).max(1);
        let Some(batch) = self.peers.get_mut(destination).map(|peer| {
            peer.process_queues();
            peer.unhandled_messages().into_iter().take(max_items).collect::<Vec<Vec<u8>>>()
        }) else {
            return Vec::new();
        };

        for transient_id in &batch {
            if self.propagation_transfer_state(transient_id).is_none() {
                self.request_propagation_transfer(transient_id.clone());
            }
        }

        if !batch.is_empty() {
            self.stats.peer_sync_runs_total += 1;
            self.stats.peer_sync_items_total += batch.len();
        }

        batch
    }

    pub fn apply_peer_sync_result(
        &mut self,
        destination: &[u8; 16],
        delivered: &[Vec<u8>],
        rejected: &[Vec<u8>],
    ) -> bool {
        {
            let Some(peer) = self.peers.get_mut(destination) else {
                return false;
            };

            for transient_id in delivered {
                peer.add_handled_message(transient_id);
            }

            for transient_id in rejected {
                peer.add_unhandled_message(transient_id);
            }

            if rejected.is_empty() {
                peer.set_sync_backoff(0);
            } else {
                let next_backoff = peer.sync_backoff().saturating_add(5).min(300);
                peer.set_sync_backoff(next_backoff);
                self.stats.peer_sync_rejected_total += rejected.len();
            }
        }

        for transient_id in delivered {
            self.complete_propagation_transfer(transient_id);
        }

        for transient_id in rejected {
            self.cancel_propagation_transfer(transient_id, "peer rejected");
        }

        true
    }
}
