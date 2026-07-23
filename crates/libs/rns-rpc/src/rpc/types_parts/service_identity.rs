#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceIdentitySpec {
    pub display_name: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub metadata: JsonMap<String, JsonValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceIdentityRecord {
    pub identity: String,
    pub delivery_destination: String,
    pub public_key: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub metadata: JsonMap<String, JsonValue>,
}

pub trait ServiceIdentityBridge: Send + Sync {
    fn list_service_identities(&self) -> Result<Vec<ServiceIdentityRecord>, std::io::Error>;

    fn create_service_identity(
        &self,
        spec: ServiceIdentitySpec,
    ) -> Result<ServiceIdentityRecord, std::io::Error>;

    fn import_service_identity(
        &self,
        private_key: &[u8],
        spec: ServiceIdentitySpec,
    ) -> Result<ServiceIdentityRecord, std::io::Error>;

    fn export_service_identity(&self, identity: &str) -> Result<Vec<u8>, std::io::Error>;

    fn announce_service_identity(
        &self,
        identity: &str,
        spec: ServiceIdentitySpec,
    ) -> Result<ServiceIdentityRecord, std::io::Error>;
}

#[derive(Debug, Clone, Default)]
struct SdkIdentitySession {
    authorized_identities: HashSet<String>,
    active_identity: Option<String>,
}
