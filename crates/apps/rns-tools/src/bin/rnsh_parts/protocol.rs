use bzip2::read::BzDecoder;
use rmpv::Value;
use rns_transport::channel::{ChannelError, TypedMessage};
use std::io::Read;

pub const PROTOCOL_VERSION: u64 = 1;
pub const MSG_MAGIC: u16 = 0xAC00;
pub const MSG_NOOP: u16 = MSG_MAGIC;
pub const MSG_WINDOW_SIZE: u16 = MSG_MAGIC + 2;
pub const MSG_EXECUTE_COMMAND: u16 = MSG_MAGIC + 3;
pub const MSG_STREAM_DATA: u16 = MSG_MAGIC + 4;
pub const MSG_VERSION_INFO: u16 = MSG_MAGIC + 5;
pub const MSG_ERROR: u16 = MSG_MAGIC + 6;
pub const MSG_COMMAND_EXITED: u16 = MSG_MAGIC + 7;

const STREAM_ID_MAX: u16 = 0x3fff;
const STREAM_EOF_MASK: u16 = 0x8000;
const STREAM_COMPRESSED_MASK: u16 = 0x4000;
const STREAM_MAX_CHUNK_LEN: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoopMessage;

impl TypedMessage for NoopMessage {
    const MSG_TYPE: u16 = MSG_NOOP;

    fn encode(&self) -> Vec<u8> {
        Vec::new()
    }

    fn decode(payload: &[u8]) -> Result<Self, ChannelError> {
        if payload.is_empty() {
            Ok(Self)
        } else {
            Err(ChannelError::InvalidFrame)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfoMessage {
    pub software_version: String,
    pub protocol_version: u64,
}

impl VersionInfoMessage {
    pub fn current() -> Self {
        Self {
            software_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
        }
    }
}

impl TypedMessage for VersionInfoMessage {
    const MSG_TYPE: u16 = MSG_VERSION_INFO;

    fn encode(&self) -> Vec<u8> {
        encode_array(vec![
            Value::String(self.software_version.clone().into()),
            Value::from(self.protocol_version),
        ])
    }

    fn decode(payload: &[u8]) -> Result<Self, ChannelError> {
        let values = decode_array(payload, 2)?;
        Ok(Self {
            software_version: required_string(&values[0])?,
            protocol_version: required_u64(&values[1])?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSizeMessage {
    pub rows: Option<u64>,
    pub cols: Option<u64>,
    pub hpix: Option<u64>,
    pub vpix: Option<u64>,
}

impl TypedMessage for WindowSizeMessage {
    const MSG_TYPE: u16 = MSG_WINDOW_SIZE;

    fn encode(&self) -> Vec<u8> {
        encode_array(vec![
            encode_optional_u64(self.rows),
            encode_optional_u64(self.cols),
            encode_optional_u64(self.hpix),
            encode_optional_u64(self.vpix),
        ])
    }

    fn decode(payload: &[u8]) -> Result<Self, ChannelError> {
        let values = decode_array(payload, 4)?;
        Ok(Self {
            rows: optional_u64(&values[0])?,
            cols: optional_u64(&values[1])?,
            hpix: optional_u64(&values[2])?,
            vpix: optional_u64(&values[3])?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecuteCommandMessage {
    pub command: Option<Vec<String>>,
    pub pipe_stdin: bool,
    pub pipe_stdout: bool,
    pub pipe_stderr: bool,
    pub term: Option<String>,
    pub rows: Option<u64>,
    pub cols: Option<u64>,
    pub hpix: Option<u64>,
    pub vpix: Option<u64>,
}

impl TypedMessage for ExecuteCommandMessage {
    const MSG_TYPE: u16 = MSG_EXECUTE_COMMAND;

    fn encode(&self) -> Vec<u8> {
        encode_array(vec![
            self.command
                .as_ref()
                .map(|items| {
                    Value::Array(
                        items.iter().cloned().map(|item| Value::String(item.into())).collect(),
                    )
                })
                .unwrap_or(Value::Nil),
            Value::Boolean(self.pipe_stdin),
            Value::Boolean(self.pipe_stdout),
            Value::Boolean(self.pipe_stderr),
            Value::Nil,
            self.term.clone().map(|value| Value::String(value.into())).unwrap_or(Value::Nil),
            encode_optional_u64(self.rows),
            encode_optional_u64(self.cols),
            encode_optional_u64(self.hpix),
            encode_optional_u64(self.vpix),
        ])
    }

    fn decode(payload: &[u8]) -> Result<Self, ChannelError> {
        let values = decode_array(payload, 10)?;
        Ok(Self {
            command: optional_string_array(&values[0])?,
            pipe_stdin: required_bool(&values[1])?,
            pipe_stdout: required_bool(&values[2])?,
            pipe_stderr: required_bool(&values[3])?,
            term: optional_string(&values[5])?,
            rows: optional_u64(&values[6])?,
            cols: optional_u64(&values[7])?,
            hpix: optional_u64(&values[8])?,
            vpix: optional_u64(&values[9])?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamDataMessage {
    pub stream_id: u16,
    pub data: Vec<u8>,
    pub eof: bool,
    pub compressed: bool,
}

impl StreamDataMessage {
    pub fn new(
        stream_id: u16,
        data: impl Into<Vec<u8>>,
        eof: bool,
        compressed: bool,
    ) -> Result<Self, ChannelError> {
        if stream_id > STREAM_ID_MAX {
            return Err(ChannelError::InvalidFrame);
        }
        Ok(Self { stream_id, data: data.into(), eof, compressed })
    }
}

impl TypedMessage for StreamDataMessage {
    const MSG_TYPE: u16 = MSG_STREAM_DATA;

    fn encode(&self) -> Vec<u8> {
        let mut header = self.stream_id & STREAM_ID_MAX;
        if self.eof {
            header |= STREAM_EOF_MASK;
        }
        if self.compressed {
            header |= STREAM_COMPRESSED_MASK;
        }
        let mut encoded = Vec::with_capacity(2 + self.data.len());
        encoded.extend_from_slice(&header.to_be_bytes());
        encoded.extend_from_slice(&self.data);
        encoded
    }

    fn decode(payload: &[u8]) -> Result<Self, ChannelError> {
        if payload.len() < 2 {
            return Err(ChannelError::InvalidFrame);
        }
        let header = u16::from_be_bytes([payload[0], payload[1]]);
        let eof = (header & STREAM_EOF_MASK) != 0;
        let compressed = (header & STREAM_COMPRESSED_MASK) != 0;
        let stream_id = header & STREAM_ID_MAX;
        let mut data = payload[2..].to_vec();
        if compressed {
            let mut decoded = Vec::new();
            BzDecoder::new(data.as_slice())
                .take((STREAM_MAX_CHUNK_LEN + 1) as u64)
                .read_to_end(&mut decoded)
                .map_err(|_| ChannelError::InvalidFrame)?;
            if decoded.len() > STREAM_MAX_CHUNK_LEN {
                return Err(ChannelError::InvalidFrame);
            }
            data = decoded;
        }
        Ok(Self { stream_id, data, eof, compressed })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorMessage {
    pub message: String,
    pub fatal: bool,
}

impl ErrorMessage {
    pub fn fatal(message: impl Into<String>) -> Self {
        Self { message: message.into(), fatal: true }
    }
}

impl TypedMessage for ErrorMessage {
    const MSG_TYPE: u16 = MSG_ERROR;

    fn encode(&self) -> Vec<u8> {
        encode_array(vec![
            Value::String(self.message.clone().into()),
            Value::Boolean(self.fatal),
            Value::Nil,
        ])
    }

    fn decode(payload: &[u8]) -> Result<Self, ChannelError> {
        let values = decode_array(payload, 3)?;
        Ok(Self { message: required_string(&values[0])?, fatal: required_bool(&values[1])? })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandExitedMessage {
    pub return_code: i64,
}

impl TypedMessage for CommandExitedMessage {
    const MSG_TYPE: u16 = MSG_COMMAND_EXITED;

    fn encode(&self) -> Vec<u8> {
        encode_value(&Value::from(self.return_code))
    }

    fn decode(payload: &[u8]) -> Result<Self, ChannelError> {
        let value = decode_value(payload)?;
        let return_code = value.as_i64().ok_or(ChannelError::InvalidFrame)?;
        Ok(Self { return_code })
    }
}

fn encode_array(values: Vec<Value>) -> Vec<u8> {
    encode_value(&Value::Array(values))
}

fn encode_value(value: &Value) -> Vec<u8> {
    rmp_serde::to_vec(value).expect("rnsh MessagePack values are serializable")
}

fn decode_value(payload: &[u8]) -> Result<Value, ChannelError> {
    rmp_serde::from_slice(payload).map_err(|_| ChannelError::InvalidFrame)
}

fn decode_array(payload: &[u8], expected_len: usize) -> Result<Vec<Value>, ChannelError> {
    match decode_value(payload)? {
        Value::Array(values) if values.len() == expected_len => Ok(values),
        _ => Err(ChannelError::InvalidFrame),
    }
}

fn required_string(value: &Value) -> Result<String, ChannelError> {
    value.as_str().map(ToOwned::to_owned).ok_or(ChannelError::InvalidFrame)
}

fn optional_string(value: &Value) -> Result<Option<String>, ChannelError> {
    if value.is_nil() {
        Ok(None)
    } else {
        required_string(value).map(Some)
    }
}

fn optional_string_array(value: &Value) -> Result<Option<Vec<String>>, ChannelError> {
    if value.is_nil() {
        return Ok(None);
    }
    let Value::Array(values) = value else {
        return Err(ChannelError::InvalidFrame);
    };
    values.iter().map(required_string).collect::<Result<Vec<_>, _>>().map(Some)
}

fn required_bool(value: &Value) -> Result<bool, ChannelError> {
    value.as_bool().ok_or(ChannelError::InvalidFrame)
}

fn optional_u64(value: &Value) -> Result<Option<u64>, ChannelError> {
    if value.is_nil() {
        Ok(None)
    } else {
        value.as_u64().map(Some).ok_or(ChannelError::InvalidFrame)
    }
}

fn required_u64(value: &Value) -> Result<u64, ChannelError> {
    value.as_u64().ok_or(ChannelError::InvalidFrame)
}

fn encode_optional_u64(value: Option<u64>) -> Value {
    value.map(Value::from).unwrap_or(Value::Nil)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rnsh_message_types_match_python_protocol_numbers() {
        assert_eq!(NoopMessage::MSG_TYPE, 0xAC00);
        assert_eq!(WindowSizeMessage::MSG_TYPE, 0xAC02);
        assert_eq!(ExecuteCommandMessage::MSG_TYPE, 0xAC03);
        assert_eq!(StreamDataMessage::MSG_TYPE, 0xAC04);
        assert_eq!(VersionInfoMessage::MSG_TYPE, 0xAC05);
        assert_eq!(ErrorMessage::MSG_TYPE, 0xAC06);
        assert_eq!(CommandExitedMessage::MSG_TYPE, 0xAC07);
    }

    #[test]
    fn execute_command_roundtrips_nil_and_optional_fields() {
        let message = ExecuteCommandMessage {
            command: Some(vec!["/bin/echo".to_string(), "hello".to_string()]),
            pipe_stdin: true,
            pipe_stdout: true,
            pipe_stderr: false,
            term: Some("xterm".to_string()),
            rows: Some(24),
            cols: Some(80),
            hpix: None,
            vpix: None,
        };
        assert_eq!(ExecuteCommandMessage::decode(&message.encode()).expect("decode"), message);
    }

    #[test]
    fn command_exit_roundtrips_negative_status() {
        let message = CommandExitedMessage { return_code: -1 };
        assert_eq!(CommandExitedMessage::decode(&message.encode()).expect("decode"), message);
    }

    #[test]
    fn command_exit_roundtrips_zero_status() {
        let message = CommandExitedMessage { return_code: 0 };
        assert_eq!(CommandExitedMessage::decode(&message.encode()).expect("decode"), message);
    }

    #[test]
    fn stream_data_matches_python_header_and_decodes_compressed_payload() {
        let message = StreamDataMessage::new(2, b"output".to_vec(), true, false).expect("message");
        assert_eq!(message.encode(), [0x80, 0x02, b'o', b'u', b't', b'p', b'u', b't']);

        let compressed = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        let mut compressed = compressed;
        std::io::Write::write_all(&mut compressed, b"compressed output").expect("write");
        let compressed = compressed.finish().expect("finish");
        let encoded = StreamDataMessage::new(1, compressed, false, true).expect("message").encode();
        let decoded = StreamDataMessage::decode(&encoded).expect("decode");
        assert_eq!(decoded.stream_id, 1);
        assert_eq!(decoded.data, b"compressed output");
        assert!(decoded.compressed);
    }

    #[test]
    fn malformed_messages_fail_closed() {
        assert!(VersionInfoMessage::decode(&[0x91, 0x01]).is_err());
        assert!(ExecuteCommandMessage::decode(&[0x90]).is_err());
        assert!(CommandExitedMessage::decode(&[0xc0]).is_err());
    }
}
