#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub profile: String,
    pub rpc: Option<String>,
    pub transport: Option<String>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self { profile: "default".to_string(), rpc: None, transport: None }
    }
}
