#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum ManagedInterfaceKey {
    Configured(String),
    Discovered([u8; 32]),
}

impl ManagedInterfaceKey {
    pub(super) fn monitor_id(&self) -> String {
        match self {
            Self::Configured(key) => format!("configured:{key}"),
            Self::Discovered(endpoint) => format!("discovered:{}", hex::encode(endpoint)),
        }
    }

    pub(super) fn configured_name(&self) -> Option<&str> {
        match self {
            Self::Configured(name) => Some(name),
            Self::Discovered(_) => None,
        }
    }
}

impl From<String> for ManagedInterfaceKey {
    fn from(key: String) -> Self {
        Self::Configured(key)
    }
}

impl From<&str> for ManagedInterfaceKey {
    fn from(key: &str) -> Self {
        Self::Configured(key.to_owned())
    }
}
