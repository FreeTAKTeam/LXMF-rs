use crate::error::{code, ErrorCategory, SdkError};
use crate::types::Ack;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MobileBleEventKind {
    Connected,
    Disconnected,
    Notification,
    WriteComplete,
    Error,
    Timeout,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MobileBleEvent {
    pub sequence_no: u64,
    pub session_id: String,
    #[serde(default)]
    pub operation_id: Option<String>,
    pub kind: MobileBleEventKind,
    #[serde(default)]
    pub payload: Option<Vec<u8>>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileBleCapabilities {
    pub supports_background_restore: bool,
    pub supports_write_without_response: bool,
    pub supports_operation_cancel: bool,
    pub max_notification_queue: usize,
    pub max_payload_bytes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileBleSessionDescriptor {
    pub session_id: String,
    pub peripheral_id: String,
    pub negotiated_mtu: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileBleConnectRequest {
    pub operation_id: String,
    pub peripheral_id: String,
    pub service_uuid: String,
    pub write_char_uuid: String,
    pub notify_char_uuid: String,
    pub connect_timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileBleWriteRequest {
    pub operation_id: String,
    pub session_id: String,
    pub payload: Vec<u8>,
    pub require_response: bool,
    pub write_timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileBleWriteAck {
    pub operation_id: String,
    pub bytes_written: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileBleReadRequest {
    pub operation_id: String,
    pub session_id: String,
    pub max_bytes: usize,
    pub read_timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileBleReadResult {
    pub operation_id: String,
    pub payload: Vec<u8>,
}

pub trait MobileBleHostAdapter: Send + Sync {
    fn capabilities(&self) -> MobileBleCapabilities;

    fn connect(&self, req: MobileBleConnectRequest)
        -> Result<MobileBleSessionDescriptor, SdkError>;

    fn disconnect(&self, session_id: &str) -> Result<Ack, SdkError>;

    fn write(&self, req: MobileBleWriteRequest) -> Result<MobileBleWriteAck, SdkError>;

    fn read(&self, req: MobileBleReadRequest) -> Result<MobileBleReadResult, SdkError>;

    fn poll_event(&self, timeout_ms: u64) -> Result<Option<MobileBleEvent>, SdkError>;

    fn cancel_operation(&self, _operation_id: &str) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.mobile_ble_cancel"))
    }
}

pub fn validate_event_sequence(events: &[MobileBleEvent]) -> Result<(), SdkError> {
    let mut last_seq = None;
    let mut connected_sessions = BTreeSet::new();

    for event in events {
        if let Some(previous) = last_seq {
            if event.sequence_no <= previous {
                return Err(SdkError::new(
                    code::VALIDATION_INVALID_ARGUMENT,
                    ErrorCategory::Validation,
                    "mobile BLE event sequence must be strictly increasing",
                )
                .with_user_actionable(true)
                .with_detail("sequence_no", serde_json::json!(event.sequence_no))
                .with_detail("previous_sequence_no", serde_json::json!(previous)));
            }
        }
        last_seq = Some(event.sequence_no);

        match event.kind {
            MobileBleEventKind::Connected => {
                connected_sessions.insert(event.session_id.clone());
            }
            MobileBleEventKind::Notification
            | MobileBleEventKind::WriteComplete
            | MobileBleEventKind::Disconnected => {
                if !connected_sessions.contains(event.session_id.as_str()) {
                    return Err(SdkError::new(
                        code::VALIDATION_INVALID_ARGUMENT,
                        ErrorCategory::Validation,
                        "mobile BLE session event observed before connection",
                    )
                    .with_user_actionable(true)
                    .with_detail("session_id", serde_json::json!(event.session_id))
                    .with_detail("event_kind", serde_json::json!(event.kind)));
                }
                if matches!(event.kind, MobileBleEventKind::Disconnected) {
                    connected_sessions.remove(event.session_id.as_str());
                }
            }
            MobileBleEventKind::Error | MobileBleEventKind::Timeout => {}
        }
    }

    Ok(())
}

pub fn validate_capabilities(capabilities: &MobileBleCapabilities) -> Result<(), SdkError> {
    if capabilities.max_notification_queue == 0 {
        return Err(SdkError::new(
            code::VALIDATION_INVALID_ARGUMENT,
            ErrorCategory::Validation,
            "mobile BLE max_notification_queue must be greater than zero",
        )
        .with_user_actionable(true)
        .with_detail(
            "max_notification_queue",
            serde_json::json!(capabilities.max_notification_queue),
        ));
    }
    if capabilities.max_payload_bytes == 0 {
        return Err(SdkError::new(
            code::VALIDATION_INVALID_ARGUMENT,
            ErrorCategory::Validation,
            "mobile BLE max_payload_bytes must be greater than zero",
        )
        .with_user_actionable(true)
        .with_detail("max_payload_bytes", serde_json::json!(capabilities.max_payload_bytes)));
    }
    Ok(())
}

pub fn validate_event_payload_bounds(
    events: &[MobileBleEvent],
    capabilities: &MobileBleCapabilities,
) -> Result<(), SdkError> {
    validate_capabilities(capabilities)?;
    for event in events {
        if matches!(event.kind, MobileBleEventKind::WriteComplete | MobileBleEventKind::Timeout)
            && !event.operation_id.as_deref().is_some_and(|value| !value.trim().is_empty())
        {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "mobile BLE operation event is missing operation_id",
            )
            .with_user_actionable(true)
            .with_detail("event_kind", serde_json::json!(event.kind))
            .with_detail("sequence_no", serde_json::json!(event.sequence_no)));
        }

        if matches!(event.kind, MobileBleEventKind::Notification)
            && event
                .payload
                .as_ref()
                .is_some_and(|payload| payload.len() > capabilities.max_payload_bytes)
        {
            let payload_len = event.payload.as_ref().map(|payload| payload.len()).unwrap_or(0);
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "mobile BLE notification payload exceeds max_payload_bytes",
            )
            .with_user_actionable(true)
            .with_detail("payload_len", serde_json::json!(payload_len))
            .with_detail("max_payload_bytes", serde_json::json!(capabilities.max_payload_bytes))
            .with_detail("sequence_no", serde_json::json!(event.sequence_no)));
        }
    }
    Ok(())
}
