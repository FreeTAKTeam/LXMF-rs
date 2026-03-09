use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct OperationId(String);

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
            Self::DuplicateAlias {
                alias,
                existing_id,
                conflicting_id,
            } => write!(
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
        let canonical = self.canonicalize(id_or_alias)?;
        self.by_id.get(&canonical).map(|index| &self.entries[*index])
    }

    pub fn canonicalize(&self, id_or_alias: impl AsRef<str>) -> Option<OperationId> {
        let value = id_or_alias.as_ref();
        if let Some(index) = self.by_id.get(value).copied() {
            return Some(self.entries[index].id.clone());
        }
        self.aliases.get(value).cloned()
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

    fn build_indexes(
        entries: &[OperationEntry],
    ) -> Result<(BTreeMap<OperationId, usize>, BTreeMap<String, OperationId>), RegistryError> {
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
        Ok(Self {
            entries: wire.entries,
            by_id,
            aliases,
        })
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
        .with_required_capability("sdk.capability.identity"),
        OperationEntry::new(
            "app.contact.list",
            "identity",
            OperationKind::Query,
            TransportVariant::Rpc,
            "List contacts for a selected identity.",
        )
        .with_alias("sdk_identity_contact_list_v2")
        .with_required_capability("sdk.capability.identity"),
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
        OperationEntry, OperationKind, OperationRegistry, RegistryError, TransportVariant,
    };

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
            registry
                .canonicalize("sdk_status_v2")
                .expect("canonical delivery status id")
                .as_str(),
            "app.delivery.status"
        );
        assert!(registry.supports("sdk_snapshot_v2"));
    }
}
