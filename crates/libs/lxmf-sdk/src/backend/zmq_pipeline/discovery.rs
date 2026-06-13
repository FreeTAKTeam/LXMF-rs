use super::{SdkError, ZmqPipelineBackendClient};
use crate::app::PeerDirectoryEntry;
use crate::backend::SdkBackend;
use crate::domain::{ContactListRequest, PresenceListRequest};
use std::collections::BTreeMap;

impl ZmqPipelineBackendClient {
    pub fn peer_directory(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<PeerDirectoryEntry>, SdkError> {
        self.peer_directory_since(limit, None)
    }

    pub fn peer_directory_since(
        &self,
        limit: Option<usize>,
        min_last_seen_ts_ms: Option<i64>,
    ) -> Result<Vec<PeerDirectoryEntry>, SdkError> {
        let mut entries = BTreeMap::<String, PeerDirectoryEntry>::new();

        for contact in self.collect_contact_records(limit)? {
            let has_display_name = contact.display_name.is_some();
            entries.insert(
                contact.identity.0.clone(),
                PeerDirectoryEntry {
                    peer_id: contact.identity.0,
                    display_name: contact.display_name,
                    name_source: has_display_name.then(|| "contact".to_owned()),
                    trust_level: Some(contact.trust_level),
                    bootstrap: contact.bootstrap,
                    online: false,
                    last_seen_ts_ms: None,
                    first_seen_ts_ms: None,
                    seen_count: 0,
                    metadata: contact.metadata,
                    extensions: contact.extensions,
                },
            );
        }

        for presence in self.collect_presence_records(limit, min_last_seen_ts_ms)? {
            let entry =
                entries.entry(presence.peer_id.clone()).or_insert_with(|| PeerDirectoryEntry {
                    peer_id: presence.peer_id.clone(),
                    display_name: presence.name.clone(),
                    name_source: presence.name_source.clone(),
                    trust_level: presence.trust_level.clone(),
                    bootstrap: presence.bootstrap.unwrap_or(false),
                    online: true,
                    last_seen_ts_ms: Some(presence.last_seen_ts_ms),
                    first_seen_ts_ms: Some(presence.first_seen_ts_ms),
                    seen_count: presence.seen_count,
                    metadata: BTreeMap::new(),
                    extensions: presence.extensions.clone(),
                });
            entry.online = true;
            entry.last_seen_ts_ms = Some(presence.last_seen_ts_ms);
            entry.first_seen_ts_ms = Some(presence.first_seen_ts_ms);
            entry.seen_count = presence.seen_count;
            if entry.display_name.is_none() {
                entry.display_name = presence.name;
            }
            if entry.name_source.is_none() {
                entry.name_source = presence.name_source;
            }
            if entry.trust_level.is_none() {
                entry.trust_level = presence.trust_level;
            }
            if !entry.bootstrap {
                entry.bootstrap = presence.bootstrap.unwrap_or(false);
            }
            for (key, value) in presence.extensions {
                entry.extensions.entry(key).or_insert(value);
            }
        }

        let mut values = entries.into_values().collect::<Vec<_>>();
        if let Some(limit) = limit {
            values.truncate(limit);
        }
        Ok(values)
    }

    fn collect_contact_records(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<crate::domain::ContactRecord>, SdkError> {
        let mut contacts = Vec::new();
        let mut cursor = None;

        loop {
            let page = self.identity_contact_list(ContactListRequest {
                cursor: cursor.clone(),
                limit,
                extensions: BTreeMap::new(),
            })?;
            contacts.extend(page.contacts);
            match page.next_cursor {
                Some(next_cursor) if cursor.as_deref() != Some(next_cursor.as_str()) => {
                    cursor = Some(next_cursor);
                }
                _ => break,
            }
        }

        Ok(contacts)
    }

    fn collect_presence_records(
        &self,
        limit: Option<usize>,
        min_last_seen_ts_ms: Option<i64>,
    ) -> Result<Vec<crate::domain::PresenceRecord>, SdkError> {
        let mut peers = Vec::new();
        let mut cursor = None;

        loop {
            let page = self.identity_presence_list(PresenceListRequest {
                cursor: cursor.clone(),
                limit,
                min_last_seen_ts_ms,
                extensions: BTreeMap::new(),
            })?;
            peers.extend(page.peers);
            match page.next_cursor {
                Some(next_cursor) if cursor.as_deref() != Some(next_cursor.as_str()) => {
                    cursor = Some(next_cursor);
                }
                _ => break,
            }
        }

        Ok(peers)
    }
}
