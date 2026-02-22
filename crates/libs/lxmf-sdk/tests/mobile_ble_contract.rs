use lxmf_sdk::{
    validate_mobile_ble_capabilities, validate_mobile_ble_event_payload_bounds,
    validate_mobile_ble_event_sequence, MobileBleCapabilities, MobileBleConnectRequest,
    MobileBleEvent, MobileBleEventKind, MobileBleHostAdapter, MobileBleReadRequest,
    MobileBleReadResult, MobileBleSessionDescriptor, MobileBleWriteAck, MobileBleWriteRequest,
};

struct DefaultCancelAdapter;

impl MobileBleHostAdapter for DefaultCancelAdapter {
    fn capabilities(&self) -> MobileBleCapabilities {
        MobileBleCapabilities {
            supports_background_restore: true,
            supports_write_without_response: true,
            supports_operation_cancel: false,
            max_notification_queue: 64,
            max_payload_bytes: 247,
        }
    }

    fn connect(
        &self,
        _req: MobileBleConnectRequest,
    ) -> Result<MobileBleSessionDescriptor, lxmf_sdk::SdkError> {
        Err(lxmf_sdk::SdkError::capability_disabled("sdk.capability.mobile_ble_connect"))
    }

    fn disconnect(&self, _session_id: &str) -> Result<lxmf_sdk::Ack, lxmf_sdk::SdkError> {
        Err(lxmf_sdk::SdkError::capability_disabled("sdk.capability.mobile_ble_disconnect"))
    }

    fn write(&self, _req: MobileBleWriteRequest) -> Result<MobileBleWriteAck, lxmf_sdk::SdkError> {
        Err(lxmf_sdk::SdkError::capability_disabled("sdk.capability.mobile_ble_write"))
    }

    fn read(&self, _req: MobileBleReadRequest) -> Result<MobileBleReadResult, lxmf_sdk::SdkError> {
        Err(lxmf_sdk::SdkError::capability_disabled("sdk.capability.mobile_ble_read"))
    }

    fn poll_event(&self, _timeout_ms: u64) -> Result<Option<MobileBleEvent>, lxmf_sdk::SdkError> {
        Ok(None)
    }
}

#[test]
fn mobile_ble_sequence_validation_accepts_ordered_session_flow() {
    let events = vec![
        MobileBleEvent {
            sequence_no: 1,
            session_id: "session-1".to_string(),
            operation_id: Some("connect-1".to_string()),
            kind: MobileBleEventKind::Connected,
            payload: None,
            error: None,
        },
        MobileBleEvent {
            sequence_no: 2,
            session_id: "session-1".to_string(),
            operation_id: Some("write-1".to_string()),
            kind: MobileBleEventKind::WriteComplete,
            payload: None,
            error: None,
        },
        MobileBleEvent {
            sequence_no: 3,
            session_id: "session-1".to_string(),
            operation_id: Some("notify-1".to_string()),
            kind: MobileBleEventKind::Notification,
            payload: Some(vec![1, 2, 3]),
            error: None,
        },
        MobileBleEvent {
            sequence_no: 4,
            session_id: "session-1".to_string(),
            operation_id: Some("disconnect-1".to_string()),
            kind: MobileBleEventKind::Disconnected,
            payload: None,
            error: None,
        },
    ];

    validate_mobile_ble_event_sequence(&events).expect("ordered event sequence should pass");
}

#[test]
fn mobile_ble_sequence_validation_rejects_non_monotonic_sequence_numbers() {
    let events = vec![
        MobileBleEvent {
            sequence_no: 4,
            session_id: "session-1".to_string(),
            operation_id: None,
            kind: MobileBleEventKind::Connected,
            payload: None,
            error: None,
        },
        MobileBleEvent {
            sequence_no: 4,
            session_id: "session-1".to_string(),
            operation_id: None,
            kind: MobileBleEventKind::Notification,
            payload: Some(vec![7]),
            error: None,
        },
    ];

    let err =
        validate_mobile_ble_event_sequence(&events).expect_err("duplicate sequence must fail");
    assert_eq!(
        err.machine_code,
        lxmf_sdk::error_code::VALIDATION_INVALID_ARGUMENT,
        "non-monotonic events should return validation error"
    );
}

#[test]
fn mobile_ble_sequence_validation_rejects_notification_before_connect() {
    let events = vec![MobileBleEvent {
        sequence_no: 1,
        session_id: "session-1".to_string(),
        operation_id: None,
        kind: MobileBleEventKind::Notification,
        payload: Some(vec![5]),
        error: None,
    }];

    let err = validate_mobile_ble_event_sequence(&events)
        .expect_err("notification before connect must fail");
    assert_eq!(
        err.machine_code,
        lxmf_sdk::error_code::VALIDATION_INVALID_ARGUMENT,
        "session ordering violation should return validation error"
    );
}

#[test]
fn mobile_ble_capability_validation_rejects_zero_limits() {
    let capabilities = MobileBleCapabilities {
        supports_background_restore: true,
        supports_write_without_response: true,
        supports_operation_cancel: false,
        max_notification_queue: 0,
        max_payload_bytes: 0,
    };

    let err = validate_mobile_ble_capabilities(&capabilities).expect_err("zero limits should fail");
    assert_eq!(err.machine_code, lxmf_sdk::error_code::VALIDATION_INVALID_ARGUMENT);
}

#[test]
fn mobile_ble_payload_bound_validation_rejects_oversized_notifications() {
    let capabilities = MobileBleCapabilities {
        supports_background_restore: true,
        supports_write_without_response: true,
        supports_operation_cancel: false,
        max_notification_queue: 32,
        max_payload_bytes: 4,
    };
    let events = vec![
        MobileBleEvent {
            sequence_no: 1,
            session_id: "session-1".to_string(),
            operation_id: Some("connect-1".to_string()),
            kind: MobileBleEventKind::Connected,
            payload: None,
            error: None,
        },
        MobileBleEvent {
            sequence_no: 2,
            session_id: "session-1".to_string(),
            operation_id: Some("notify-1".to_string()),
            kind: MobileBleEventKind::Notification,
            payload: Some(vec![1, 2, 3, 4, 5]),
            error: None,
        },
    ];

    let err = validate_mobile_ble_event_payload_bounds(&events, &capabilities)
        .expect_err("payload over max should fail");
    assert_eq!(err.machine_code, lxmf_sdk::error_code::VALIDATION_INVALID_ARGUMENT);
}

#[test]
fn mobile_ble_default_cancel_operation_is_capability_gated() {
    let adapter = DefaultCancelAdapter;
    let err = adapter
        .cancel_operation("write-1")
        .expect_err("default cancel_operation should be capability-gated");
    assert_eq!(err.machine_code, lxmf_sdk::error_code::CAPABILITY_DISABLED);
}
