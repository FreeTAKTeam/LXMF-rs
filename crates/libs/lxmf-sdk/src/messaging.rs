use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerState {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageMethod {
    Direct,
    Opportunistic,
    Propagated,
    Resource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageState {
    Queued,
    PathRequested,
    LinkEstablishing,
    Sending,
    SentDirect,
    SentToPropagation,
    Delivered,
    Failed,
    TimedOut,
    Cancelled,
    Received,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncPhase {
    Idle,
    PathRequested,
    LinkEstablishing,
    RequestSent,
    Receiving,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnounceRecord {
    pub destination_hex: String,
    pub identity_hex: String,
    pub destination_kind: String,
    pub app_data: String,
    pub display_name: Option<String>,
    pub hops: u8,
    pub interface_hex: String,
    pub received_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerRecord {
    pub destination_hex: String,
    pub identity_hex: Option<String>,
    pub lxmf_destination_hex: Option<String>,
    pub display_name: Option<String>,
    pub app_data: Option<String>,
    pub state: PeerState,
    pub last_seen_at_ms: u64,
    pub announce_last_seen_at_ms: Option<u64>,
    pub lxmf_last_seen_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub conversation_id: String,
    pub peer_destination_hex: String,
    pub peer_display_name: Option<String>,
    pub last_message_preview: Option<String>,
    pub last_message_at_ms: u64,
    pub unread_count: u32,
    pub last_message_state: Option<MessageState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageRecord {
    pub message_id_hex: String,
    pub conversation_id: String,
    pub direction: MessageDirection,
    pub destination_hex: String,
    pub source_hex: Option<String>,
    pub title: Option<String>,
    pub body_utf8: String,
    pub method: MessageMethod,
    pub state: MessageState,
    pub detail: Option<String>,
    pub sent_at_ms: Option<u64>,
    pub received_at_ms: Option<u64>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatus {
    pub phase: SyncPhase,
    pub active_propagation_node_hex: Option<String>,
    pub requested_at_ms: Option<u64>,
    pub completed_at_ms: Option<u64>,
    pub messages_received: u32,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub destination_hex: String,
    pub body_utf8: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOutboundMessage {
    pub request: SendMessageRequest,
    pub message_id_hex: String,
}

#[derive(Debug, Clone, Default)]
pub struct MessagingStore {
    announce_records: HashMap<String, AnnounceRecord>,
    message_records: HashMap<String, MessageRecord>,
    message_order: Vec<String>,
    outbound_messages: HashMap<String, StoredOutboundMessage>,
    sync_status: SyncStatus,
}

impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            phase: SyncPhase::Idle,
            active_propagation_node_hex: None,
            requested_at_ms: None,
            completed_at_ms: None,
            messages_received: 0,
            detail: None,
        }
    }
}

impl MessagingStore {
    pub fn conversation_id_for(destination_hex: &str) -> String {
        destination_hex.trim().to_ascii_lowercase()
    }

    pub fn record_announce(&mut self, record: AnnounceRecord) {
        self.announce_records.insert(record.destination_hex.clone(), record);
    }

    pub fn list_announces(&self) -> Vec<AnnounceRecord> {
        let mut records = self.announce_records.values().cloned().collect::<Vec<_>>();
        records.sort_by_key(|record| Reverse(record.received_at_ms));
        records
    }

    pub fn list_peers<'a, I>(&self, connected_destinations: I) -> Vec<PeerRecord>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let connected = connected_destinations
            .into_iter()
            .map(|value| value.to_ascii_lowercase())
            .collect::<HashSet<_>>();

        let mut app_dest_by_identity = HashMap::<String, String>::new();
        let mut lxmf_dest_by_identity = HashMap::<String, String>::new();
        let mut app_records = HashMap::<String, AnnounceRecord>::new();
        let mut lxmf_records = HashMap::<String, AnnounceRecord>::new();

        for record in self.announce_records.values() {
            if record.destination_kind == "app" {
                app_dest_by_identity
                    .insert(record.identity_hex.clone(), record.destination_hex.clone());
                app_records.insert(record.destination_hex.clone(), record.clone());
            } else if record.destination_kind == "lxmf_delivery" {
                lxmf_dest_by_identity
                    .insert(record.identity_hex.clone(), record.destination_hex.clone());
                lxmf_records.insert(record.destination_hex.clone(), record.clone());
            }
        }

        let mut peers = Vec::<PeerRecord>::new();
        for (destination_hex, app_record) in app_records {
            let identity_hex = Some(app_record.identity_hex.clone());
            let lxmf_destination_hex = identity_hex
                .as_ref()
                .and_then(|identity| lxmf_dest_by_identity.get(identity).cloned());
            let connected_match = connected.contains(destination_hex.as_str())
                || lxmf_destination_hex
                    .as_ref()
                    .is_some_and(|value| connected.contains(value.as_str()));
            peers.push(PeerRecord {
                destination_hex: destination_hex.clone(),
                identity_hex,
                lxmf_destination_hex: lxmf_destination_hex.clone(),
                display_name: lxmf_destination_hex
                    .as_ref()
                    .and_then(|value| lxmf_records.get(value))
                    .and_then(|record| record.display_name.clone()),
                app_data: Some(app_record.app_data.clone()),
                state: if connected_match { PeerState::Connected } else { PeerState::Disconnected },
                last_seen_at_ms: app_record.received_at_ms.max(
                    lxmf_destination_hex
                        .as_ref()
                        .and_then(|value| lxmf_records.get(value))
                        .map(|record| record.received_at_ms)
                        .unwrap_or(0),
                ),
                announce_last_seen_at_ms: Some(app_record.received_at_ms),
                lxmf_last_seen_at_ms: lxmf_destination_hex
                    .as_ref()
                    .and_then(|value| lxmf_records.get(value))
                    .map(|record| record.received_at_ms),
            });
        }

        for (identity_hex, lxmf_destination_hex) in lxmf_dest_by_identity {
            if app_dest_by_identity.contains_key(identity_hex.as_str()) {
                continue;
            }
            if let Some(record) = lxmf_records.get(&lxmf_destination_hex) {
                peers.push(PeerRecord {
                    destination_hex: lxmf_destination_hex.clone(),
                    identity_hex: Some(identity_hex),
                    lxmf_destination_hex: Some(lxmf_destination_hex.clone()),
                    display_name: record.display_name.clone(),
                    app_data: None,
                    state: if connected.contains(lxmf_destination_hex.as_str()) {
                        PeerState::Connected
                    } else {
                        PeerState::Disconnected
                    },
                    last_seen_at_ms: record.received_at_ms,
                    announce_last_seen_at_ms: None,
                    lxmf_last_seen_at_ms: Some(record.received_at_ms),
                });
            }
        }

        peers.sort_by_key(|peer| Reverse(peer.last_seen_at_ms));
        peers
    }

    pub fn peer_for_identity<'a, I>(
        &self,
        identity_hex: &str,
        connected_destinations: I,
    ) -> Option<PeerRecord>
    where
        I: IntoIterator<Item = &'a str>,
    {
        self.list_peers(connected_destinations)
            .into_iter()
            .find(|peer| peer.identity_hex.as_deref() == Some(identity_hex))
    }

    pub fn upsert_message(&mut self, message: MessageRecord) -> bool {
        let is_new = !self.message_records.contains_key(message.message_id_hex.as_str());
        self.message_records.insert(message.message_id_hex.clone(), message.clone());
        if is_new {
            self.message_order.push(message.message_id_hex);
        }
        is_new
    }

    pub fn get_message(&self, message_id_hex: &str) -> Option<MessageRecord> {
        self.message_records.get(message_id_hex).cloned()
    }

    pub fn update_message(
        &mut self,
        message_id_hex: &str,
        state: MessageState,
        detail: Option<String>,
        updated_at_ms: u64,
    ) -> Option<MessageRecord> {
        let record = self.message_records.get_mut(message_id_hex)?;
        record.state = state;
        record.detail = detail;
        record.updated_at_ms = updated_at_ms;
        Some(record.clone())
    }

    pub fn list_messages(&self, conversation_id: Option<&str>) -> Vec<MessageRecord> {
        let mut out = Vec::<MessageRecord>::new();
        for message_id_hex in &self.message_order {
            let Some(record) = self.message_records.get(message_id_hex).cloned() else {
                continue;
            };
            if conversation_id.is_some_and(|value| record.conversation_id != value) {
                continue;
            }
            out.push(record);
        }
        out.sort_by(|left, right| {
            let left_time = left.received_at_ms.or(left.sent_at_ms).unwrap_or(left.updated_at_ms);
            let right_time =
                right.received_at_ms.or(right.sent_at_ms).unwrap_or(right.updated_at_ms);
            left_time.cmp(&right_time)
        });
        out
    }

    pub fn list_conversations<'a, I>(&self, connected_destinations: I) -> Vec<ConversationRecord>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let peers = self.list_peers(connected_destinations);
        let mut peer_map = HashMap::<String, PeerRecord>::new();
        for peer in peers {
            peer_map.insert(peer.destination_hex.clone(), peer.clone());
            if let Some(lxmf_destination_hex) = peer.lxmf_destination_hex.clone() {
                peer_map.insert(lxmf_destination_hex, peer);
            }
        }

        let records = self.list_messages(None);
        let mut by_conversation = HashMap::<String, ConversationRecord>::new();
        for record in records {
            let entry =
                by_conversation.entry(record.conversation_id.clone()).or_insert_with(|| {
                    ConversationRecord {
                        conversation_id: record.conversation_id.clone(),
                        peer_destination_hex: record.destination_hex.clone(),
                        peer_display_name: peer_map
                            .get(&record.destination_hex)
                            .and_then(peer_display_name_for),
                        last_message_preview: None,
                        last_message_at_ms: 0,
                        unread_count: 0,
                        last_message_state: None,
                    }
                });

            let event_time =
                record.received_at_ms.or(record.sent_at_ms).unwrap_or(record.updated_at_ms);
            if event_time >= entry.last_message_at_ms {
                entry.peer_destination_hex = record.destination_hex.clone();
                entry.peer_display_name =
                    peer_map.get(&record.destination_hex).and_then(peer_display_name_for);
                entry.last_message_preview = message_preview(record.body_utf8.as_str());
                entry.last_message_at_ms = event_time;
                entry.last_message_state = Some(record.state);
            }
            if matches!(record.direction, MessageDirection::Inbound) {
                entry.unread_count = entry.unread_count.saturating_add(1);
            }
        }

        let mut out = by_conversation.into_values().collect::<Vec<_>>();
        out.sort_by_key(|conversation| Reverse(conversation.last_message_at_ms));
        out
    }

    pub fn store_outbound(&mut self, outbound: StoredOutboundMessage) {
        self.outbound_messages.insert(outbound.message_id_hex.clone(), outbound);
    }

    pub fn outbound(&self, message_id_hex: &str) -> Option<StoredOutboundMessage> {
        self.outbound_messages.get(message_id_hex).cloned()
    }

    pub fn set_active_propagation_node(&mut self, destination_hex: Option<String>) -> SyncStatus {
        self.sync_status.active_propagation_node_hex = destination_hex;
        self.sync_status.clone()
    }

    pub fn sync_status(&self) -> SyncStatus {
        self.sync_status.clone()
    }

    pub fn update_sync_status<F>(&mut self, apply: F) -> SyncStatus
    where
        F: FnOnce(&mut SyncStatus),
    {
        apply(&mut self.sync_status);
        self.sync_status.clone()
    }
}

fn message_preview(body_utf8: &str) -> Option<String> {
    let trimmed = body_utf8.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(80).collect())
}

fn peer_display_name_for(peer: &PeerRecord) -> Option<String> {
    peer.display_name
        .clone()
        .or_else(|| peer.identity_hex.clone())
        .or_else(|| Some(peer.destination_hex.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_projection_merges_app_and_lxmf_announces() {
        let mut store = MessagingStore::default();
        store.record_announce(AnnounceRecord {
            destination_hex: "appdest".into(),
            identity_hex: "identity".into(),
            destination_kind: "app".into(),
            app_data: "R3AKT".into(),
            display_name: None,
            hops: 1,
            interface_hex: "iface".into(),
            received_at_ms: 10,
        });
        store.record_announce(AnnounceRecord {
            destination_hex: "lxmfdest".into(),
            identity_hex: "identity".into(),
            destination_kind: "lxmf_delivery".into(),
            app_data: "chat".into(),
            display_name: Some("Alice".into()),
            hops: 1,
            interface_hex: "iface".into(),
            received_at_ms: 20,
        });

        let peers = store.list_peers(["lxmfdest"]);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].destination_hex, "appdest");
        assert_eq!(peers[0].lxmf_destination_hex.as_deref(), Some("lxmfdest"));
        assert_eq!(peers[0].display_name.as_deref(), Some("Alice"));
        assert_eq!(peers[0].state, PeerState::Connected);
    }

    #[test]
    fn conversation_projection_uses_lxmf_destination_for_peer_lookup() {
        let mut store = MessagingStore::default();
        store.record_announce(AnnounceRecord {
            destination_hex: "appdest".into(),
            identity_hex: "identity".into(),
            destination_kind: "app".into(),
            app_data: "R3AKT".into(),
            display_name: None,
            hops: 1,
            interface_hex: "iface".into(),
            received_at_ms: 10,
        });
        store.record_announce(AnnounceRecord {
            destination_hex: "lxmfdest".into(),
            identity_hex: "identity".into(),
            destination_kind: "lxmf_delivery".into(),
            app_data: "chat".into(),
            display_name: Some("Alice".into()),
            hops: 1,
            interface_hex: "iface".into(),
            received_at_ms: 20,
        });
        store.upsert_message(MessageRecord {
            message_id_hex: "msg".into(),
            conversation_id: "lxmfdest".into(),
            direction: MessageDirection::Outbound,
            destination_hex: "lxmfdest".into(),
            source_hex: None,
            title: None,
            body_utf8: "hello".into(),
            method: MessageMethod::Direct,
            state: MessageState::Delivered,
            detail: None,
            sent_at_ms: Some(30),
            received_at_ms: None,
            updated_at_ms: 30,
        });

        let conversations = store.list_conversations(std::iter::empty::<&str>());
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].peer_display_name.as_deref(), Some("Alice"));
    }
}
