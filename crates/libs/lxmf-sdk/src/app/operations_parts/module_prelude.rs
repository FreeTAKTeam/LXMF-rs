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
