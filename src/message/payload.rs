use serde::{Deserialize, Serialize};

use crate::error::LxmfError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Payload {
    pub timestamp: f64,
    pub content: Option<String>,
    pub title: Option<String>,
    pub fields: Option<serde_json::Value>,
}

impl Payload {
    pub fn new(
        timestamp: f64,
        content: Option<String>,
        title: Option<String>,
        fields: Option<serde_json::Value>,
    ) -> Self {
        Self {
            timestamp,
            content,
            title,
            fields,
        }
    }

    pub fn to_msgpack(&self) -> Result<Vec<u8>, LxmfError> {
        let list = (
            self.timestamp,
            self.content.clone(),
            self.title.clone(),
            self.fields.clone(),
        );
        rmp_serde::to_vec(&list).map_err(|e| LxmfError::Encode(e.to_string()))
    }

    pub fn from_msgpack(bytes: &[u8]) -> Result<Self, LxmfError> {
        let (timestamp, content, title, fields): (
            f64,
            Option<String>,
            Option<String>,
            Option<serde_json::Value>,
        ) = rmp_serde::from_slice(bytes)
            .map_err(|e| LxmfError::Decode(e.to_string()))?;
        Ok(Self {
            timestamp,
            content,
            title,
            fields,
        })
    }
}
