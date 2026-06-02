use super::discovery::{
    BootstrapRequest, Contact, ContactPage, ContactUpdate, Identity, PeerDirectoryEntry, Presence,
    PresencePage,
};
use super::errors::Error;
use super::node::Client;
use crate::domain::{ContactListRequest, PresenceListRequest, TopicListRequest};
use crate::SdkBackend;
use std::collections::BTreeMap;

impl<B: SdkBackend> Client<B> {
    pub fn identities(&self) -> Result<Vec<Identity>, Error> {
        let identities = self.backend.identity_list().map_err(Error::from)?;
        Ok(identities.into_iter().map(Identity::from).collect())
    }

    pub fn announce_now(&self) -> Result<(), Error> {
        self.backend.identity_announce_now().map_err(Error::from)?;
        Ok(())
    }

    pub fn contacts(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<ContactPage, Error> {
        let result = self
            .backend
            .identity_contact_list(ContactListRequest {
                cursor,
                limit,
                extensions: BTreeMap::new(),
            })
            .map_err(Error::from)?;
        Ok(ContactPage::from(result))
    }

    pub fn presence(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<PresencePage, Error> {
        let result = self
            .backend
            .identity_presence_list(PresenceListRequest {
                cursor,
                limit,
                extensions: BTreeMap::new(),
            })
            .map_err(Error::from)?;
        Ok(PresencePage::from(result))
    }

    pub fn update_contact(&self, update: ContactUpdate) -> Result<Contact, Error> {
        let contact = self.backend.identity_contact_update(update.into()).map_err(Error::from)?;
        Ok(Contact::from(contact))
    }

    pub fn bootstrap_identity(&self, request: BootstrapRequest) -> Result<Contact, Error> {
        let contact = self.backend.identity_bootstrap(request.into()).map_err(Error::from)?;
        Ok(Contact::from(contact))
    }

    pub fn peer_directory(&self, limit: Option<usize>) -> Result<Vec<PeerDirectoryEntry>, Error> {
        let mut entries = BTreeMap::<String, PeerDirectoryEntry>::new();

        for contact in self.collect_contacts(limit)? {
            entries.insert(
                contact.identity.clone(),
                PeerDirectoryEntry {
                    peer_id: contact.identity.clone(),
                    display_name: contact.display_name.clone(),
                    name_source: Some("contact".to_owned()),
                    trust_level: Some(contact.trust_level.clone()),
                    bootstrap: contact.bootstrap,
                    online: false,
                    last_seen_ts_ms: None,
                    first_seen_ts_ms: None,
                    seen_count: 0,
                    metadata: contact.metadata.clone(),
                    extensions: contact.extensions.clone(),
                },
            );
        }

        for presence in self.collect_presence(limit)? {
            let entry =
                entries.entry(presence.peer_id.clone()).or_insert_with(|| PeerDirectoryEntry {
                    peer_id: presence.peer_id.clone(),
                    display_name: presence.display_name.clone(),
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
                entry.display_name = presence.display_name.clone();
            }
            if entry.name_source.is_none() {
                entry.name_source = presence.name_source.clone();
            }
            if entry.trust_level.is_none() {
                entry.trust_level = presence.trust_level.clone();
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

    pub(crate) fn find_contact(&self, identity: &str) -> Result<Option<Contact>, Error> {
        Ok(self.collect_contacts(None)?.into_iter().find(|contact| contact.identity == identity))
    }

    pub(crate) fn find_topic_by_path(
        &self,
        topic_path: &str,
        page_size: usize,
    ) -> Result<Option<crate::domain::TopicRecord>, Error> {
        let mut cursor = None;
        loop {
            let page = self
                .backend
                .topic_list(TopicListRequest {
                    cursor: cursor.clone(),
                    limit: Some(page_size),
                    extensions: BTreeMap::new(),
                })
                .map_err(Error::from)?;
            if let Some(found) = page.topics.into_iter().find(|topic| {
                topic.topic_path.as_ref().map(|path| path.0.as_str()) == Some(topic_path)
            }) {
                return Ok(Some(found));
            }
            match page.next_cursor {
                Some(next_cursor) if cursor.as_deref() != Some(next_cursor.as_str()) => {
                    cursor = Some(next_cursor);
                }
                _ => return Ok(None),
            }
        }
    }

    pub(crate) fn collect_contacts(&self, limit: Option<usize>) -> Result<Vec<Contact>, Error> {
        let mut contacts = Vec::new();
        let mut cursor = None;

        loop {
            let page = self.contacts(cursor.clone(), limit)?;
            contacts.extend(page.contacts);
            if let Some(limit) = limit {
                if contacts.len() >= limit {
                    contacts.truncate(limit);
                    break;
                }
            }
            match page.next_cursor {
                Some(next_cursor) if cursor.as_deref() != Some(next_cursor.as_str()) => {
                    cursor = Some(next_cursor);
                }
                _ => break,
            }
        }

        Ok(contacts)
    }

    pub(crate) fn collect_presence(&self, limit: Option<usize>) -> Result<Vec<Presence>, Error> {
        let mut peers = Vec::new();
        let mut cursor = None;

        loop {
            let page = self.presence(cursor.clone(), limit)?;
            peers.extend(page.peers);
            if let Some(limit) = limit {
                if peers.len() >= limit {
                    peers.truncate(limit);
                    break;
                }
            }
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
