use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use crate::error::LxmfError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Payload {
    pub timestamp: f64,
    pub content: Option<ByteBuf>,
    pub title: Option<ByteBuf>,
    pub fields: Option<rmpv::Value>,
    pub stamp: Option<ByteBuf>,
}

impl Payload {
    pub fn new(
        timestamp: f64,
        content: Option<Vec<u8>>,
        title: Option<Vec<u8>>,
        fields: Option<rmpv::Value>,
        stamp: Option<Vec<u8>>,
    ) -> Self {
        Self {
            timestamp,
            content: content.map(ByteBuf::from),
            title: title.map(ByteBuf::from),
            fields,
            stamp: stamp.map(ByteBuf::from),
        }
    }

    pub fn to_msgpack(&self) -> Result<Vec<u8>, LxmfError> {
        if let Some(stamp) = &self.stamp {
            let list = (
                self.timestamp,
                self.title.clone(),
                self.content.clone(),
                self.fields.clone(),
                stamp.clone(),
            );
            rmp_serde::to_vec(&list).map_err(|e| LxmfError::Encode(e.to_string()))
        } else {
            self.to_msgpack_without_stamp()
        }
    }

    pub fn to_msgpack_without_stamp(&self) -> Result<Vec<u8>, LxmfError> {
        let list = (self.timestamp, self.title.clone(), self.content.clone(), self.fields.clone());
        rmp_serde::to_vec(&list).map_err(|e| LxmfError::Encode(e.to_string()))
    }

    pub fn from_msgpack(bytes: &[u8]) -> Result<Self, LxmfError> {
        let value = rmp_serde::from_slice::<rmpv::Value>(bytes)
            .map_err(|e| LxmfError::Decode(e.to_string()))?;
        let rmpv::Value::Array(items) = value else {
            return Err(LxmfError::Decode("invalid payload structure".into()));
        };
        if items.len() < 4 || items.len() > 5 {
            return Err(LxmfError::Decode("invalid payload length".into()));
        }
        let timestamp = items
            .first()
            .and_then(|value| value.as_f64())
            .ok_or_else(|| LxmfError::Decode("invalid payload timestamp".into()))?;
        let title = value_to_bytes(items.get(1), "title")?.map(ByteBuf::from);
        let content = value_to_bytes(items.get(2), "content")?.map(ByteBuf::from);
        let fields = match items.get(3) {
            Some(rmpv::Value::Nil) | None => None,
            Some(value) => Some(value.clone()),
        };
        let stamp = if items.len() == 5 {
            value_to_bytes(items.get(4), "stamp")?.map(ByteBuf::from)
        } else {
            None
        };
        Ok(Self { timestamp, content, title, fields, stamp })
    }
}

fn value_to_bytes(value: Option<&rmpv::Value>, field: &str) -> Result<Option<Vec<u8>>, LxmfError> {
    match value {
        Some(rmpv::Value::Binary(bin)) => Ok(Some(bin.clone())),
        Some(rmpv::Value::String(text)) => text
            .as_str()
            .map(|s| Some(s.as_bytes().to_vec()))
            .ok_or_else(|| LxmfError::Decode(format!("invalid payload {field} string"))),
        Some(rmpv::Value::Nil) | None => Ok(None),
        _ => Err(LxmfError::Decode(format!("invalid payload {field}"))),
    }
}
