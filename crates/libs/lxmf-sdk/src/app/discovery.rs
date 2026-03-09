use crate::domain::{
    ContactListResult, ContactRecord, ContactUpdateRequest, IdentityBootstrapRequest,
    IdentityBundle, IdentityRef, PresenceListResult, PresenceRecord, TrustLevel,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Identity {
    pub identity: String,
    pub public_key: String,
    pub display_name: Option<String>,
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Contact {
    pub identity: String,
    pub display_name: Option<String>,
    pub trust_level: TrustLevel,
    pub bootstrap: bool,
    pub updated_ts_ms: u64,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Presence {
    pub peer_id: String,
    pub last_seen_ts_ms: i64,
    pub first_seen_ts_ms: i64,
    pub seen_count: u64,
    pub display_name: Option<String>,
    pub name_source: Option<String>,
    pub trust_level: Option<TrustLevel>,
    pub bootstrap: Option<bool>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ContactPage {
    pub contacts: Vec<Contact>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PresencePage {
    pub peers: Vec<Presence>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PeerDirectoryEntry {
    pub peer_id: String,
    pub display_name: Option<String>,
    pub name_source: Option<String>,
    pub trust_level: Option<TrustLevel>,
    pub bootstrap: bool,
    pub online: bool,
    pub last_seen_ts_ms: Option<i64>,
    pub first_seen_ts_ms: Option<i64>,
    pub seen_count: u64,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ContactUpdate {
    pub identity: String,
    pub display_name: Option<String>,
    pub trust_level: Option<TrustLevel>,
    pub bootstrap: Option<bool>,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

impl ContactUpdate {
    pub fn new(identity: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
            display_name: None,
            trust_level: None,
            bootstrap: None,
            metadata: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }

    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    pub fn with_trust_level(mut self, trust_level: TrustLevel) -> Self {
        self.trust_level = Some(trust_level);
        self
    }

    pub fn with_bootstrap(mut self, bootstrap: bool) -> Self {
        self.bootstrap = Some(bootstrap);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct BootstrapRequest {
    pub identity: String,
    pub auto_sync: bool,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

impl BootstrapRequest {
    pub fn new(identity: impl Into<String>) -> Self {
        Self { identity: identity.into(), auto_sync: true, extensions: BTreeMap::new() }
    }

    pub fn with_auto_sync(mut self, auto_sync: bool) -> Self {
        self.auto_sync = auto_sync;
        self
    }
}

impl From<IdentityBundle> for Identity {
    fn from(value: IdentityBundle) -> Self {
        Self {
            identity: value.identity.0,
            public_key: value.public_key,
            display_name: value.display_name,
            capabilities: value.capabilities,
            extensions: value.extensions,
        }
    }
}

impl From<ContactRecord> for Contact {
    fn from(value: ContactRecord) -> Self {
        Self {
            identity: value.identity.0,
            display_name: value.display_name,
            trust_level: value.trust_level,
            bootstrap: value.bootstrap,
            updated_ts_ms: value.updated_ts_ms,
            metadata: value.metadata,
            extensions: value.extensions,
        }
    }
}

impl From<PresenceRecord> for Presence {
    fn from(value: PresenceRecord) -> Self {
        Self {
            peer_id: value.peer_id,
            last_seen_ts_ms: value.last_seen_ts_ms,
            first_seen_ts_ms: value.first_seen_ts_ms,
            seen_count: value.seen_count,
            display_name: value.name,
            name_source: value.name_source,
            trust_level: value.trust_level,
            bootstrap: value.bootstrap,
            extensions: value.extensions,
        }
    }
}

impl From<ContactListResult> for ContactPage {
    fn from(value: ContactListResult) -> Self {
        Self {
            contacts: value.contacts.into_iter().map(Contact::from).collect(),
            next_cursor: value.next_cursor,
        }
    }
}

impl From<PresenceListResult> for PresencePage {
    fn from(value: PresenceListResult) -> Self {
        Self {
            peers: value.peers.into_iter().map(Presence::from).collect(),
            next_cursor: value.next_cursor,
        }
    }
}

impl From<ContactUpdate> for ContactUpdateRequest {
    fn from(value: ContactUpdate) -> Self {
        Self {
            identity: IdentityRef(value.identity),
            display_name: value.display_name,
            trust_level: value.trust_level,
            bootstrap: value.bootstrap,
            metadata: value.metadata,
            extensions: value.extensions,
        }
    }
}

impl From<BootstrapRequest> for IdentityBootstrapRequest {
    fn from(value: BootstrapRequest) -> Self {
        Self {
            identity: IdentityRef(value.identity),
            auto_sync: value.auto_sync,
            extensions: value.extensions,
        }
    }
}
