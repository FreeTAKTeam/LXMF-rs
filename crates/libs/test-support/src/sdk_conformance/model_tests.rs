use super::*;
use lxmf_sdk::{CancelResult, DeliveryState, LxmfSdkManualTick, TickBudget};
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeliveryModelState {
    Queued,
    Dispatching,
    InFlight,
    Sent,
    Delivered,
    Failed,
    Cancelled,
    Expired,
    Rejected,
    Unknown,
}

impl DeliveryModelState {
    fn is_terminal(self) -> bool {
        matches!(
            self,
            DeliveryModelState::Delivered
                | DeliveryModelState::Failed
                | DeliveryModelState::Cancelled
                | DeliveryModelState::Expired
                | DeliveryModelState::Rejected
        )
    }
}

#[derive(Clone, Copy, Debug)]
enum DeliveryOp {
    Receipt(&'static str),
    Cancel,
}

fn map_receipt_status(status: &str) -> DeliveryModelState {
    let normalized = status.trim().to_ascii_lowercase();
    if normalized.starts_with("sent") {
        return DeliveryModelState::Sent;
    }
    match normalized.as_str() {
        "queued" => DeliveryModelState::Queued,
        "dispatching" => DeliveryModelState::Dispatching,
        "in_flight" | "inflight" => DeliveryModelState::InFlight,
        "delivered" => DeliveryModelState::Delivered,
        "failed" => DeliveryModelState::Failed,
        "cancelled" => DeliveryModelState::Cancelled,
        "expired" => DeliveryModelState::Expired,
        "rejected" => DeliveryModelState::Rejected,
        _ => DeliveryModelState::Unknown,
    }
}

fn apply_delivery_model(
    state: DeliveryModelState,
    op: DeliveryOp,
) -> (DeliveryModelState, Option<CancelResult>) {
    match op {
        DeliveryOp::Receipt(status) => {
            if state.is_terminal() {
                (state, None)
            } else {
                (map_receipt_status(status), None)
            }
        }
        DeliveryOp::Cancel => {
            if state.is_terminal() {
                (state, Some(CancelResult::AlreadyTerminal))
            } else {
                (state, Some(CancelResult::TooLateToCancel))
            }
        }
    }
}

fn as_delivery_state(state: DeliveryModelState) -> DeliveryState {
    match state {
        DeliveryModelState::Queued => DeliveryState::Queued,
        DeliveryModelState::Dispatching => DeliveryState::Dispatching,
        DeliveryModelState::InFlight => DeliveryState::InFlight,
        DeliveryModelState::Sent => DeliveryState::Sent,
        DeliveryModelState::Delivered => DeliveryState::Delivered,
        DeliveryModelState::Failed => DeliveryState::Failed,
        DeliveryModelState::Cancelled => DeliveryState::Cancelled,
        DeliveryModelState::Expired => DeliveryState::Expired,
        DeliveryModelState::Rejected => DeliveryState::Rejected,
        DeliveryModelState::Unknown => DeliveryState::Unknown,
    }
}

fn assert_invalid_state(err: lxmf_sdk::SdkError, context: &str) {
    assert_eq!(err.machine_code, "SDK_RUNTIME_INVALID_STATE", "{}", context);
}

fn tick_budget(max_work_items: usize, max_duration_ms: Option<u64>) -> TickBudget {
    serde_json::from_value(json!({
        "max_work_items": max_work_items,
        "max_duration_ms": max_duration_ms,
    }))
    .expect("deserialize TickBudget")
}

#[test]
fn sdk_model_client_lifecycle_contract_transitions() {
    let harness = RpcHarness::new();
    let client = harness.client();

    let err = client.send(send_request("lifecycle-new", None)).expect_err("send in New must fail");
    assert_invalid_state(err, "send in New");
    let err = client.snapshot().expect_err("snapshot in New must fail");
    assert_invalid_state(err, "snapshot in New");

    client.start(base_start_request()).expect("start");
    let message_id = client.send(send_request("lifecycle-running", None)).expect("send in Running");
    client.status(message_id.clone()).expect("status in Running");
    client.poll_events(None, 8).expect("poll in Running");
    client.snapshot().expect("snapshot in Running");
    client.configure(0, overflow_patch()).expect("configure in Running");
    let tick_err = client
        .tick(tick_budget(16, Some(5)))
        .expect_err("tick in Running should fail without manual_tick capability");
    assert_eq!(tick_err.machine_code, "SDK_CAPABILITY_DISABLED");
    client.cancel(message_id.clone()).expect("cancel in Running");

    client.shutdown(lxmf_sdk::ShutdownMode::Graceful).expect("shutdown");

    let err = client.send(send_request("lifecycle-stopped", None)).expect_err("send in Stopped");
    assert_invalid_state(err, "send in Stopped");
    let err = client
        .status(message_id.clone())
        .expect_err("status in Stopped must fail with invalid state");
    assert_invalid_state(err, "status in Stopped");
    let err = client.configure(1, overflow_patch()).expect_err("configure in Stopped must fail");
    assert_invalid_state(err, "configure in Stopped");
    let err = client.poll_events(None, 8).expect_err("poll in Stopped must fail");
    assert_invalid_state(err, "poll in Stopped");
    let err = client.snapshot().expect_err("snapshot in Stopped must fail");
    assert_invalid_state(err, "snapshot in Stopped");
    let err = client.tick(tick_budget(8, Some(3))).expect_err("tick in Stopped must fail");
    assert_invalid_state(err, "tick in Stopped");
    let err = client.cancel(message_id).expect_err("cancel in Stopped must fail");
    assert_invalid_state(err, "cancel in Stopped");

    let second_shutdown =
        client.shutdown(lxmf_sdk::ShutdownMode::Graceful).expect("shutdown idempotency");
    assert!(second_shutdown.accepted, "shutdown must remain idempotent after Stopped");
}

#[test]
fn sdk_model_delivery_state_machine_sticky_terminals_match_reference() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let scenarios: &[&[DeliveryOp]] = &[
        &[DeliveryOp::Receipt("queued"), DeliveryOp::Cancel, DeliveryOp::Receipt("delivered")],
        &[DeliveryOp::Receipt("queued"), DeliveryOp::Receipt("delivered"), DeliveryOp::Cancel],
        &[
            DeliveryOp::Receipt("queued"),
            DeliveryOp::Receipt("failed"),
            DeliveryOp::Receipt("sent: direct"),
        ],
    ];

    for (idx, scenario) in scenarios.iter().enumerate() {
        let message =
            client.send(send_request(&format!("model-delivery-{idx}"), None)).expect("send");
        let message_id = message.0.clone();
        let mut model_state = DeliveryModelState::Sent;

        let initial = client.status(message.clone()).expect("status").expect("status snapshot");
        assert_eq!(initial.state, DeliveryState::Sent);
        assert!(!initial.terminal, "sent should be non-terminal when receipt_terminality exists");

        for op in *scenario {
            let (next_state, expected_cancel) = apply_delivery_model(model_state, *op);
            match op {
                DeliveryOp::Receipt(status) => {
                    let response = harness.rpc_call(
                        "record_receipt",
                        Some(json!({
                            "message_id": message_id,
                            "status": status,
                        })),
                    );
                    assert!(
                        response.error.is_none(),
                        "record_receipt failed for status {status}: {:?}",
                        response.error
                    );
                }
                DeliveryOp::Cancel => {
                    let cancel = client.cancel(message.clone()).expect("cancel");
                    assert_eq!(
                        expected_cancel,
                        Some(cancel),
                        "cancel result mismatch for scenario {idx} at state {:?}",
                        model_state
                    );
                }
            }

            model_state = next_state;
            let snapshot =
                client.status(message.clone()).expect("status").expect("status snapshot");
            assert_eq!(
                snapshot.state,
                as_delivery_state(model_state),
                "delivery state mismatch for scenario {idx}"
            );
            assert_eq!(
                snapshot.terminal,
                model_state.is_terminal(),
                "terminality mismatch for scenario {idx}"
            );
        }
    }
}
