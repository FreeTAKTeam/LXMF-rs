use crate::constants::FIELD_COMMANDS;
use crate::LxmfError;
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
    pub command_id: u8,
    pub payload: Vec<u8>,
}

impl CommandEntry {
    pub fn from_text(command_id: u8, payload: &str) -> Self {
        Self { command_id, payload: payload.as_bytes().to_vec() }
    }

    pub fn from_bytes(command_id: u8, payload: Vec<u8>) -> Self {
        Self { command_id, payload }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WireFields {
    entries: BTreeMap<u8, rmpv::Value>,
}

impl WireFields {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_field(&mut self, field_id: u8, value: rmpv::Value) -> &mut Self {
        self.entries.insert(field_id, value);
        self
    }

    pub fn set_commands<I>(&mut self, commands: I) -> &mut Self
    where
        I: IntoIterator<Item = CommandEntry>,
    {
        let mut out = Vec::new();
        for command in commands {
            let entry = rmpv::Value::Map(vec![(
                rmpv::Value::Integer((command.command_id as i64).into()),
                rmpv::Value::Binary(command.payload),
            )]);
            out.push(entry);
        }
        self.entries.insert(FIELD_COMMANDS, rmpv::Value::Array(out));
        self
    }

    pub fn to_rmpv(&self) -> rmpv::Value {
        let mut entries = Vec::with_capacity(self.entries.len());
        for (field_id, value) in &self.entries {
            entries.push((rmpv::Value::Integer((*field_id as i64).into()), value.clone()));
        }
        rmpv::Value::Map(entries)
    }

    pub fn encode_msgpack(&self) -> Result<Vec<u8>, LxmfError> {
        rmp_serde::to_vec(&self.to_rmpv()).map_err(|err| LxmfError::Encode(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandEntry, WireFields};
    use crate::constants::FIELD_COMMANDS;

    #[test]
    fn commands_encode_with_integer_field_ids() {
        let mut fields = WireFields::new();
        fields.set_commands(vec![
            CommandEntry::from_text(0x01, "ping"),
            CommandEntry::from_bytes(0x02, vec![0xAA, 0xBB]),
        ]);

        let rmpv::Value::Map(entries) = fields.to_rmpv() else { panic!("expected map") };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.as_i64(), Some(FIELD_COMMANDS as i64));
        let Some(cmds) = entries[0].1.as_array().cloned() else {
            panic!("commands array expected")
        };
        assert_eq!(cmds.len(), 2);
    }
}
