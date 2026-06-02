use super::envelope::EnvelopeKind;
use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct OperationId(String);

type OperationIndexes = (BTreeMap<OperationId, usize>, BTreeMap<String, OperationId>);

impl OperationId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for OperationId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for OperationId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for OperationId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for OperationId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OperationKind {
    Query,
    Command,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TransportVariant {
    App,
    Rpc,
    LegacyRpc,
    Extension,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TransportFamily {
    Local,
    Rpc,
    Legacy,
    Extension,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct OperationEntry {
    pub id: OperationId,
    pub group: String,
    pub kind: OperationKind,
    pub transport_variant: TransportVariant,
    pub description: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}

impl OperationEntry {
    pub fn new(
        id: impl Into<OperationId>,
        group: impl Into<String>,
        kind: OperationKind,
        transport_variant: TransportVariant,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            group: group.into(),
            kind,
            transport_variant,
            description: description.into(),
            aliases: Vec::new(),
            required_capabilities: Vec::new(),
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    pub fn with_required_capability(mut self, capability: impl Into<String>) -> Self {
        self.required_capabilities.push(capability.into());
        self
    }

    pub fn expected_envelope_kind(&self) -> EnvelopeKind {
        match self.kind {
            OperationKind::Query => EnvelopeKind::Query,
            OperationKind::Command => EnvelopeKind::Command,
        }
    }

    pub fn accepts_envelope_kind(&self, kind: &EnvelopeKind) -> bool {
        matches!(
            (kind, &self.kind),
            (EnvelopeKind::Query, OperationKind::Query)
                | (EnvelopeKind::Command, OperationKind::Command)
        )
    }

    pub fn transport_family(&self) -> TransportFamily {
        match self.transport_variant {
            TransportVariant::App => TransportFamily::Local,
            TransportVariant::Rpc => TransportFamily::Rpc,
            TransportVariant::LegacyRpc => TransportFamily::Legacy,
            TransportVariant::Extension => TransportFamily::Extension,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedOperation<'a> {
    pub entry: &'a OperationEntry,
    pub canonical_id: &'a OperationId,
    pub alias: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RegistryError {
    DuplicateOperationId { id: OperationId },
    DuplicateAlias { alias: String, existing_id: OperationId, conflicting_id: OperationId },
    AliasConflictsWithOperationId { alias: String, operation_id: OperationId },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateOperationId { id } => {
                write!(f, "duplicate operation id '{}'", id.as_str())
            }
            Self::DuplicateAlias { alias, existing_id, conflicting_id } => write!(
                f,
                "duplicate alias '{}' for '{}' and '{}'",
                alias,
                existing_id.as_str(),
                conflicting_id.as_str()
            ),
            Self::AliasConflictsWithOperationId { alias, operation_id } => write!(
                f,
                "alias '{}' conflicts with canonical operation id '{}'",
                alias,
                operation_id.as_str()
            ),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, Default)]
pub struct OperationRegistry {
    entries: Vec<OperationEntry>,
    #[serde(skip)]
    by_id: BTreeMap<OperationId, usize>,
    #[serde(skip)]
    aliases: BTreeMap<String, OperationId>,
}

impl OperationRegistry {
    pub fn new(entries: impl IntoIterator<Item = OperationEntry>) -> Result<Self, RegistryError> {
        let entries = entries.into_iter().collect::<Vec<_>>();
        let (by_id, aliases) = Self::build_indexes(&entries)?;
        Ok(Self { entries, by_id, aliases })
    }

    pub fn built_in() -> &'static Self {
        static REGISTRY: OnceLock<OperationRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            Self::new(built_in_entries()).expect("built-in app operation registry should be valid")
        })
    }

    pub fn entries(&self) -> &[OperationEntry] {
        &self.entries
    }

    pub fn get(&self, id_or_alias: impl AsRef<str>) -> Option<&OperationEntry> {
        self.resolve(id_or_alias).map(|resolved| resolved.entry)
    }

    pub fn canonicalize(&self, id_or_alias: impl AsRef<str>) -> Option<OperationId> {
        let value = id_or_alias.as_ref();
        if let Some(index) = self.by_id.get(value).copied() {
            return Some(self.entries[index].id.clone());
        }
        self.aliases.get(value).cloned()
    }

    pub fn resolve(&self, id_or_alias: impl AsRef<str>) -> Option<ResolvedOperation<'_>> {
        let value = id_or_alias.as_ref();
        let canonical = self.canonicalize(value)?;
        let index = *self.by_id.get(&canonical)?;
        Some(ResolvedOperation {
            entry: &self.entries[index],
            canonical_id: &self.entries[index].id,
            alias: (value != canonical.as_str()).then(|| value.to_owned()),
        })
    }

    pub fn entries_by_group(&self) -> BTreeMap<&str, Vec<&OperationEntry>> {
        let mut grouped = BTreeMap::<&str, Vec<&OperationEntry>>::new();
        for entry in &self.entries {
            grouped.entry(entry.group.as_str()).or_default().push(entry);
        }
        grouped
    }

    pub fn supports(&self, id_or_alias: impl AsRef<str>) -> bool {
        self.canonicalize(id_or_alias).is_some()
    }

    pub fn merged(
        &self,
        entries: impl IntoIterator<Item = OperationEntry>,
    ) -> Result<Self, RegistryError> {
        let mut merged = self.entries.clone();
        merged.extend(entries);
        Self::new(merged)
    }

    fn build_indexes(entries: &[OperationEntry]) -> Result<OperationIndexes, RegistryError> {
        let mut by_id = BTreeMap::<OperationId, usize>::new();
        let mut aliases = BTreeMap::<String, OperationId>::new();

        for (index, entry) in entries.iter().enumerate() {
            if by_id.insert(entry.id.clone(), index).is_some() {
                return Err(RegistryError::DuplicateOperationId { id: entry.id.clone() });
            }
        }

        for entry in entries {
            for alias in &entry.aliases {
                if by_id.contains_key(alias.as_str()) {
                    return Err(RegistryError::AliasConflictsWithOperationId {
                        alias: alias.clone(),
                        operation_id: OperationId::from(alias.clone()),
                    });
                }
                if let Some(existing_id) = aliases.insert(alias.clone(), entry.id.clone()) {
                    return Err(RegistryError::DuplicateAlias {
                        alias: alias.clone(),
                        existing_id,
                        conflicting_id: entry.id.clone(),
                    });
                }
            }
        }

        Ok((by_id, aliases))
    }
}

impl<'de> Deserialize<'de> for OperationRegistry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireRegistry {
            entries: Vec<OperationEntry>,
        }

        let wire = WireRegistry::deserialize(deserializer)?;
        let (by_id, aliases) =
            OperationRegistry::build_indexes(&wire.entries).map_err(serde::de::Error::custom)?;
        Ok(Self { entries: wire.entries, by_id, aliases })
    }
}

fn built_in_entries() -> Vec<OperationEntry> {
    vec![
        OperationEntry::new(
            "app.runtime.start",
            "runtime",
            OperationKind::Command,
            TransportVariant::App,
            "Start or attach to the configured runtime session.",
        )
        .with_alias("sdk_negotiate_v2")
        .with_alias("sdk_configure_v2")
        .with_alias("sdk_start_v2"),
        OperationEntry::new(
            "app.runtime.restart",
            "runtime",
            OperationKind::Command,
            TransportVariant::App,
            "Restart the runtime with a new app configuration.",
        ),
        OperationEntry::new(
            "app.runtime.stop",
            "runtime",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Stop the runtime session.",
        )
        .with_alias("sdk_shutdown_v2"),
        OperationEntry::new(
            "app.runtime.status",
            "runtime",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Return runtime status and queue counters.",
        )
        .with_alias("sdk_snapshot_v2"),
        OperationEntry::new(
            "app.delivery.send",
            "delivery",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Queue one outbound message for delivery.",
        )
        .with_alias("sdk_send_v2"),
        OperationEntry::new(
            "app.delivery.send_batch",
            "delivery",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Queue a batch of outbound messages for delivery.",
        )
        .with_alias("sdk_send_batch_v2")
        .with_required_capability("sdk.capability.batch_send"),
        OperationEntry::new(
            "app.delivery.status",
            "delivery",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Return delivery state for a specific message id.",
        )
        .with_alias("sdk_status_v2"),
        OperationEntry::new(
            "app.event.poll",
            "events",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Poll batches of runtime events.",
        )
        .with_alias("sdk_poll_events_v2"),
        OperationEntry::new(
            "app.event.subscribe",
            "events",
            OperationKind::Query,
            TransportVariant::App,
            "Subscribe to the async runtime event stream.",
        )
        .with_alias("sdk_subscribe_events_v2")
        .with_required_capability("sdk.capability.async_events"),
        OperationEntry::new(
            "app.identity.list",
            "identity",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List identities visible to the runtime.",
        )
        .with_alias("sdk_identity_list_v2")
        .with_required_capability("sdk.capability.identity_multi"),
        OperationEntry::new(
            "app.identity.announce",
            "identity",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Trigger an announce for the active identity.",
        )
        .with_alias("sdk_identity_announce_now_v2")
        .with_required_capability("sdk.capability.identity_discovery"),
        OperationEntry::new(
            "app.identity.presence.list",
            "identity",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List recently seen peers and announce-derived presence state.",
        )
        .with_alias("sdk_identity_presence_list_v2")
        .with_required_capability("sdk.capability.identity_discovery"),
        OperationEntry::new(
            "app.contact.list",
            "identity",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List contacts for a selected identity.",
        )
        .with_alias("sdk_identity_contact_list_v2")
        .with_required_capability("sdk.capability.contact_management"),
        OperationEntry::new(
            "app.contact.update",
            "identity",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Create or update a contact record for an identity.",
        )
        .with_alias("sdk_identity_contact_update_v2")
        .with_required_capability("sdk.capability.contact_management"),
        OperationEntry::new(
            "app.identity.bootstrap",
            "identity",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Bootstrap trust and optional sync state for an identity.",
        )
        .with_alias("sdk_identity_bootstrap_v2")
        .with_required_capability("sdk.capability.contact_management"),
        OperationEntry::new(
            "app.topic.create",
            "topics",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Create a topic record for collaborative app flows.",
        )
        .with_alias("sdk_topic_create_v2")
        .with_required_capability("sdk.capability.topics"),
        OperationEntry::new(
            "app.topic.get",
            "topics",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Fetch one topic record by id.",
        )
        .with_alias("sdk_topic_get_v2")
        .with_required_capability("sdk.capability.topics"),
        OperationEntry::new(
            "app.topic.list",
            "topics",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List known topics with cursor pagination.",
        )
        .with_alias("sdk_topic_list_v2")
        .with_required_capability("sdk.capability.topics"),
        OperationEntry::new(
            "app.topic.subscribe",
            "topics",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Subscribe the runtime to topic updates.",
        )
        .with_alias("sdk_topic_subscribe_v2")
        .with_required_capability("sdk.capability.topic_subscriptions"),
        OperationEntry::new(
            "app.topic.unsubscribe",
            "topics",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Remove a topic subscription from the runtime.",
        )
        .with_alias("sdk_topic_unsubscribe_v2")
        .with_required_capability("sdk.capability.topic_subscriptions"),
        OperationEntry::new(
            "app.topic.publish",
            "topics",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Publish one payload fanout to a topic.",
        )
        .with_alias("sdk_topic_publish_v2")
        .with_required_capability("sdk.capability.topic_fanout"),
        OperationEntry::new(
            "app.telemetry.query",
            "telemetry",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Query telemetry points filtered by peer, topic, and time bounds.",
        )
        .with_alias("sdk_telemetry_query_v2")
        .with_required_capability("sdk.capability.telemetry_query"),
        OperationEntry::new(
            "app.telemetry.subscribe",
            "telemetry",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Subscribe the runtime to telemetry stream updates.",
        )
        .with_alias("sdk_telemetry_subscribe_v2")
        .with_required_capability("sdk.capability.telemetry_stream"),
        OperationEntry::new(
            "app.attachment.store",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Store one attachment payload with optional topic associations.",
        )
        .with_alias("sdk_attachment_store_v2")
        .with_required_capability("sdk.capability.attachments"),
        OperationEntry::new(
            "app.attachment.get",
            "attachments",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Fetch one attachment metadata record by id.",
        )
        .with_alias("sdk_attachment_get_v2")
        .with_required_capability("sdk.capability.attachments"),
        OperationEntry::new(
            "app.attachment.list",
            "attachments",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List stored attachments with topic filtering and cursor pagination.",
        )
        .with_alias("sdk_attachment_list_v2")
        .with_required_capability("sdk.capability.attachments"),
        OperationEntry::new(
            "app.attachment.delete",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Delete one stored attachment by id.",
        )
        .with_alias("sdk_attachment_delete_v2")
        .with_required_capability("sdk.capability.attachment_delete"),
        OperationEntry::new(
            "app.attachment.associate_topic",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Associate an existing attachment with an additional topic.",
        )
        .with_alias("sdk_attachment_associate_topic_v2")
        .with_required_capability("sdk.capability.attachments"),
        OperationEntry::new(
            "app.attachment.upload_start",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Open a chunked attachment upload session.",
        )
        .with_alias("sdk_attachment_upload_start_v2")
        .with_required_capability("sdk.capability.attachment_streaming"),
        OperationEntry::new(
            "app.attachment.upload_chunk",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Append one chunk to an attachment upload session.",
        )
        .with_alias("sdk_attachment_upload_chunk_v2")
        .with_required_capability("sdk.capability.attachment_streaming"),
        OperationEntry::new(
            "app.attachment.upload_commit",
            "attachments",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Commit a completed attachment upload session.",
        )
        .with_alias("sdk_attachment_upload_commit_v2")
        .with_required_capability("sdk.capability.attachment_streaming"),
        OperationEntry::new(
            "app.attachment.download_chunk",
            "attachments",
            OperationKind::Query,
            TransportVariant::Rpc,
            "Read one chunk from a stored attachment payload.",
        )
        .with_alias("sdk_attachment_download_chunk_v2")
        .with_required_capability("sdk.capability.attachment_streaming"),
        OperationEntry::new(
            "app.marker.create",
            "markers",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Create a shared marker anchored to an optional topic.",
        )
        .with_alias("sdk_marker_create_v2")
        .with_required_capability("sdk.capability.markers"),
        OperationEntry::new(
            "app.marker.list",
            "markers",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List markers with topic filtering and cursor pagination.",
        )
        .with_alias("sdk_marker_list_v2")
        .with_required_capability("sdk.capability.markers"),
        OperationEntry::new(
            "app.marker.update_position",
            "markers",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Move an existing marker while enforcing revision checks.",
        )
        .with_alias("sdk_marker_update_position_v2")
        .with_required_capability("sdk.capability.markers"),
        OperationEntry::new(
            "app.marker.delete",
            "markers",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Delete an existing marker while enforcing revision checks.",
        )
        .with_alias("sdk_marker_delete_v2")
        .with_required_capability("sdk.capability.markers"),
        OperationEntry::new(
            "app.workflow.peer_ready",
            "workflow",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Ensure a peer contact exists and optionally announce before use.",
        )
        .with_alias("sdk_workflow_peer_ready_v2")
        .with_required_capability("sdk.capability.contact_management")
        .with_required_capability("sdk.capability.identity_discovery"),
        OperationEntry::new(
            "app.workflow.topic_sync",
            "workflow",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Ensure a topic exists, subscribe to it, and fetch a telemetry snapshot.",
        )
        .with_alias("sdk_workflow_topic_sync_v2")
        .with_required_capability("sdk.capability.topics")
        .with_required_capability("sdk.capability.topic_subscriptions")
        .with_required_capability("sdk.capability.telemetry_query"),
        OperationEntry::new(
            "app.workflow.attachment_report_publish",
            "workflow",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Ensure a topic, store an attachment, and publish a summary report.",
        )
        .with_alias("sdk_workflow_attachment_report_publish_v2")
        .with_required_capability("sdk.capability.topics")
        .with_required_capability("sdk.capability.attachments")
        .with_required_capability("sdk.capability.topic_fanout"),
        OperationEntry::new(
            "app.workflow.mission_update_send",
            "workflow",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Ensure peer and optional topic state, store attachments, and send a mission update.",
        )
        .with_alias("sdk_workflow_mission_update_send_v2")
        .with_required_capability("sdk.capability.contact_management")
        .with_required_capability("sdk.capability.topics")
        .with_required_capability("sdk.capability.attachments"),
        OperationEntry::new(
            "app.voice.session.open",
            "voice",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Open a voice signaling session for a peer.",
        )
        .with_alias("sdk_voice_session_open_v2")
        .with_required_capability("sdk.capability.voice_signaling"),
        OperationEntry::new(
            "app.voice.session.update",
            "voice",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Advance the state of a voice signaling session.",
        )
        .with_alias("sdk_voice_session_update_v2")
        .with_required_capability("sdk.capability.voice_signaling"),
        OperationEntry::new(
            "app.voice.session.close",
            "voice",
            OperationKind::Command,
            TransportVariant::Rpc,
            "Close a voice signaling session.",
        )
        .with_alias("sdk_voice_session_close_v2")
        .with_required_capability("sdk.capability.voice_signaling"),
        OperationEntry::new(
            "app.message.history.list",
            "messaging",
            OperationKind::Query,
            TransportVariant::LegacyRpc,
            "List message history records for app chat flows.",
        )
        .with_alias("list_messages"),
        OperationEntry::new(
            "app.delivery.destination_hash",
            "identity",
            OperationKind::Query,
            TransportVariant::LegacyRpc,
            "Resolve the runtime delivery destination hash.",
        )
        .with_alias("status"),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        OperationEntry, OperationKind, OperationRegistry, RegistryError, TransportFamily,
        TransportVariant,
    };
    use crate::app::EnvelopeKind;

    #[test]
    fn built_in_registry_canonicalizes_aliases() {
        let registry = OperationRegistry::built_in();
        assert_eq!(
            registry.canonicalize("sdk_identity_list_v2").expect("canonical operation id").as_str(),
            "app.identity.list"
        );
        assert_eq!(
            registry.get("sdk_poll_events_v2").expect("entry").id.as_str(),
            "app.event.poll"
        );
    }

    #[test]
    fn merged_registry_supports_custom_operations() {
        let registry = OperationRegistry::built_in()
            .merged([OperationEntry::new(
                "vendor.example.custom",
                "custom",
                OperationKind::Command,
                TransportVariant::Extension,
                "Custom vendor command.",
            )
            .with_alias("vendor/custom")])
            .expect("merged registry");

        assert!(registry.supports("vendor/custom"));
        assert_eq!(
            registry.canonicalize("vendor/custom").expect("canonical custom id").as_str(),
            "vendor.example.custom"
        );
    }

    #[test]
    fn rejects_duplicate_aliases() {
        let err = OperationRegistry::new([
            OperationEntry::new(
                "app.one",
                "test",
                OperationKind::Query,
                TransportVariant::App,
                "one",
            )
            .with_alias("dup"),
            OperationEntry::new(
                "app.two",
                "test",
                OperationKind::Query,
                TransportVariant::App,
                "two",
            )
            .with_alias("dup"),
        ])
        .expect_err("duplicate alias should fail");

        assert!(matches!(err, RegistryError::DuplicateAlias { alias, .. } if alias == "dup"));
    }

    #[test]
    fn deserialized_registry_rebuilds_lookup_indexes() {
        let json = serde_json::to_string(OperationRegistry::built_in()).expect("registry json");
        let registry: OperationRegistry = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(
            registry.canonicalize("sdk_status_v2").expect("canonical delivery status id").as_str(),
            "app.delivery.status"
        );
        assert!(registry.supports("sdk_snapshot_v2"));
    }

    #[test]
    fn resolve_reports_alias_and_transport_family() {
        let registry = OperationRegistry::built_in();
        let resolved = registry.resolve("sdk_poll_events_v2").expect("resolved alias");

        assert_eq!(resolved.canonical_id.as_str(), "app.event.poll");
        assert_eq!(resolved.alias.as_deref(), Some("sdk_poll_events_v2"));
        assert_eq!(resolved.entry.transport_family(), TransportFamily::Rpc);
        assert_eq!(resolved.entry.expected_envelope_kind(), EnvelopeKind::Query);
        assert!(resolved.entry.accepts_envelope_kind(&EnvelopeKind::Query));
        assert!(!resolved.entry.accepts_envelope_kind(&EnvelopeKind::Command));
    }

    #[test]
    fn registry_groups_entries_for_catalog_views() {
        let registry = OperationRegistry::built_in();
        let grouped = registry.entries_by_group();

        assert!(grouped.contains_key("runtime"));
        assert!(grouped.contains_key("attachments"));
        assert!(grouped.contains_key("markers"));
        assert!(grouped.contains_key("telemetry"));
        assert!(grouped.contains_key("topics"));
        assert!(grouped.contains_key("voice"));
        assert!(grouped
            .get("identity")
            .expect("identity group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.identity.list"));
        assert!(grouped
            .get("attachments")
            .expect("attachments group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.attachment.store"));
        assert!(grouped
            .get("topics")
            .expect("topics group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.topic.create"));
        assert!(grouped
            .get("telemetry")
            .expect("telemetry group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.telemetry.query"));
        assert!(grouped
            .get("markers")
            .expect("markers group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.marker.create"));
        assert!(grouped
            .get("voice")
            .expect("voice group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.voice.session.open"));
    }

    #[test]
    fn r3akt_style_catalog_aliases_roundtrip_through_registry_json() {
        let registry = OperationRegistry::new([
            OperationEntry::new(
                "mission.join",
                "Core Discovery and Session",
                OperationKind::Command,
                TransportVariant::Extension,
                "Register the sender LXMF destination with the hub connection list.",
            )
            .with_alias("POST /RCH")
            .with_alias("POST /RTH"),
            OperationEntry::new(
                "mission.marker.list",
                "Map, Markers, and Zones",
                OperationKind::Query,
                TransportVariant::Extension,
                "List mission markers.",
            )
            .with_alias("GET /api/markers"),
        ])
        .expect("registry");

        let json = serde_json::to_string(&registry).expect("registry json");
        let roundtrip: OperationRegistry = serde_json::from_str(&json).expect("roundtrip");
        let resolved = roundtrip.resolve("POST /RCH").expect("alias resolution");

        assert_eq!(resolved.canonical_id.as_str(), "mission.join");
        assert_eq!(resolved.alias.as_deref(), Some("POST /RCH"));
        assert_eq!(resolved.entry.group, "Core Discovery and Session");
    }
}
