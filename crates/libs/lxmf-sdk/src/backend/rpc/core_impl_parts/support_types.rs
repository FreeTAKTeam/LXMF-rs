#[cfg(test)]
mod tests {
    use super::RpcBackendClient;
    use crate::error::{code, ErrorCategory, SdkError};
    use crate::event::{EventBatch, EventCursor, SdkEvent, Severity};
    use crate::types::{SendRequest, TickBudget};
    use serde_json::json;
    use std::collections::{BTreeMap, VecDeque};

    fn test_event(seq_no: u64) -> SdkEvent {
        SdkEvent {
            event_id: format!("evt-{seq_no}"),
            runtime_id: "rt-1".to_owned(),
            stream_id: "stream-1".to_owned(),
            seq_no,
            contract_version: 2,
            ts_ms: seq_no * 10,
            event_type: "DeliveryStateTransition".to_owned(),
            severity: Severity::Info,
            source_component: "test".to_owned(),
            operation_id: None,
            message_id: None,
            peer_id: None,
            correlation_id: None,
            trace_id: None,
            payload: json!({}),
            extensions: BTreeMap::new(),
        }
    }

    fn test_batch(start_seq: u64, count: usize, next_cursor: &str) -> EventBatch {
        let mut events = Vec::with_capacity(count);
        for offset in 0..count {
            let seq_no = start_seq + offset as u64;
            events.push(test_event(seq_no));
        }
        EventBatch {
            events,
            next_cursor: EventCursor(next_cursor.to_owned()),
            dropped_count: 0,
            snapshot_high_watermark_seq_no: None,
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn parse_cancel_result_accepts_contract_variants() {
        assert!(matches!(
            RpcBackendClient::parse_cancel_result("Accepted"),
            Ok(crate::types::CancelResult::Accepted)
        ));
        assert!(matches!(
            RpcBackendClient::parse_cancel_result("AlreadyTerminal"),
            Ok(crate::types::CancelResult::AlreadyTerminal)
        ));
        assert!(matches!(
            RpcBackendClient::parse_cancel_result("NotFound"),
            Ok(crate::types::CancelResult::NotFound)
        ));
        assert!(matches!(
            RpcBackendClient::parse_cancel_result("TooLateToCancel"),
            Ok(crate::types::CancelResult::TooLateToCancel)
        ));
    }

    #[test]
    fn parse_cancel_result_rejects_unknown_variant() {
        let err = RpcBackendClient::parse_cancel_result("LegacyUnsupported")
            .expect_err("unknown cancel result must fail");
        assert_eq!(err.machine_code, crate::error::code::INTERNAL);
        assert_eq!(
            err.details.get("cancel_result"),
            Some(&serde_json::Value::String("LegacyUnsupported".to_owned()))
        );
    }

    #[test]
    fn send_params_preserve_delivery_options() {
        let backend = RpcBackendClient::new("127.0.0.1:65530");
        let params = backend.send_params(
            SendRequest::new(
                "source-destination",
                "target-destination",
                json!({ "title": "ops", "content": "hello" }),
            )
            .with_delivery_method("propagated")
            .with_stamp_cost(8)
            .with_include_ticket(true)
            .with_try_propagation_on_fail(true)
            .with_correlation_id("corr-rpc"),
        );

        assert_eq!(params["source"], json!("source-destination"));
        assert_eq!(params["destination"], json!("target-destination"));
        assert_eq!(params["method"], json!("propagated"));
        assert_eq!(params["stamp_cost"], json!(8));
        assert_eq!(params["include_ticket"], json!(true));
        assert_eq!(params["try_propagation_on_fail"], json!(true));
        assert!(params["fields"].is_null());
    }

    #[test]
    fn send_params_uses_only_explicit_lxmf_fields_for_wire_payload() {
        let backend = RpcBackendClient::new("127.0.0.1:65530");
        let params = backend.send_params(
            SendRequest::new(
                "source-destination",
                "target-destination",
                json!({
                    "title": "ops",
                    "content": "hello",
                    "fields": {
                        "9": [{ "command_type": "status.request" }],
                        "12": [170, 187],
                    },
                }),
            )
            .with_correlation_id("corr-rpc"),
        );

        assert_eq!(params["title"], json!("ops"));
        assert_eq!(params["content"], json!("hello"));
        assert_eq!(params["fields"]["9"][0]["command_type"], json!("status.request"));
        assert_eq!(params["fields"]["12"], json!([170, 187]));
        assert_eq!(params["fields"].get("title"), None);
        assert_eq!(params["fields"].get("content"), None);
        assert_eq!(params["fields"].get("_sdk"), None);
    }

    #[test]
    fn manual_tick_loop_is_deterministic_for_fixed_budget() {
        let expected_batches = vec![
            test_batch(1, 2, "cursor-1"),
            test_batch(3, 2, "cursor-2"),
            test_batch(5, 1, "cursor-3"),
        ];
        let mut expected_calls: Option<Vec<(Option<String>, usize)>> = None;

        for _ in 0..2 {
            let mut batches = VecDeque::from(expected_batches.clone());
            let mut calls = Vec::new();
            let (processed_items, cursor) =
                RpcBackendClient::run_manual_tick_loop(None, 5, 2, |cursor, max| {
                    calls.push((cursor.as_ref().map(|value| value.0.clone()), max));
                    batches.pop_front().ok_or_else(|| {
                        SdkError::new(
                            code::INTERNAL,
                            ErrorCategory::Internal,
                            "test batch queue exhausted unexpectedly",
                        )
                    })
                })
                .expect("manual tick loop should succeed");

            assert_eq!(processed_items, 5);
            assert_eq!(cursor, Some(EventCursor("cursor-3".to_owned())));
            match &expected_calls {
                Some(expected) => assert_eq!(&calls, expected),
                None => expected_calls = Some(calls),
            }
        }
    }

    #[test]
    fn manual_tick_loop_stops_when_backend_is_idle() {
        let mut call_count = 0usize;
        let (processed_items, cursor) =
            RpcBackendClient::run_manual_tick_loop(None, 8, 4, |_, _| {
                call_count += 1;
                Ok(EventBatch::empty(EventCursor("cursor-idle".to_owned())))
            })
            .expect("manual tick loop should succeed");

        assert_eq!(call_count, 1, "idle backend should terminate tick loop in one poll");
        assert_eq!(processed_items, 0);
        assert_eq!(cursor, Some(EventCursor("cursor-idle".to_owned())));
    }

    #[test]
    fn tick_delay_is_deterministic_for_work_and_idle_paths() {
        let budget = TickBudget::new(16).with_max_duration_ms(40);
        assert_eq!(RpcBackendClient::recommended_tick_delay_ms(&budget, 0, false), Some(40));
        assert_eq!(RpcBackendClient::recommended_tick_delay_ms(&budget, 1, false), Some(0));
        assert_eq!(RpcBackendClient::recommended_tick_delay_ms(&budget, 16, true), Some(0));

        let default_budget = TickBudget::new(4);
        assert_eq!(
            RpcBackendClient::recommended_tick_delay_ms(&default_budget, 0, false),
            Some(25)
        );
    }

    #[test]
    fn tick_impl_rejects_missing_capability_and_zero_budget() {
        let backend = RpcBackendClient::new("127.0.0.1:65530");
        let missing_capability =
            backend.tick_impl(TickBudget::new(1)).expect_err("manual tick capability required");
        assert_eq!(missing_capability.machine_code, code::CAPABILITY_DISABLED);

        backend
            .negotiated_capabilities
            .write()
            .expect("negotiated_capabilities rwlock poisoned")
            .push("sdk.capability.manual_tick".to_owned());
        let zero_budget = backend
            .tick_impl(TickBudget { max_work_items: 0, max_duration_ms: None })
            .expect_err("zero budget must fail");
        assert_eq!(zero_budget.machine_code, code::VALIDATION_INVALID_ARGUMENT);
    }
}
