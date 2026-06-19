#[tokio::test]
async fn transport_bridge_reports_deferred_stamp_queue_and_cancels_work() {
    let (daemon, _bridge, _recipient, recipient_hex) =
        test_transport_bridge_fixture_with_peer().await;

    let send = daemon
        .handle_rpc(RpcRequest {
            id: 240,
            method: "send_message_v2".into(),
            params: Some(json!({
                "id": "deferred-stamp-queue-1",
                "source": "src",
                "destination": recipient_hex,
                "title": "deferred stamp",
                "content": "body",
                "stamp_cost": 255
            })),
        })
        .expect("send");
    assert!(send.error.is_none(), "deferred stamped send should enqueue");

    let mut saw_stamp_worker = false;
    for _ in 0..40 {
        let status = daemon
            .handle_rpc(RpcRequest {
                id: 241,
                method: "sdk_status_v2".into(),
                params: Some(json!({ "message_id": "deferred-stamp-queue-1" })),
            })
            .expect("status")
            .result
            .expect("status result");
        let pipeline = &status["delivery_pipeline"];
        let stamp_queued = pipeline["stamp_queued_total"].as_u64().unwrap_or_default();
        let stamp_in_flight = pipeline["stamp_in_flight_total"].as_u64().unwrap_or_default();
        if stamp_queued + stamp_in_flight > 0 {
            saw_stamp_worker = true;
            assert_eq!(pipeline["in_flight_total"].as_u64().unwrap_or_default(), 0);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(saw_stamp_worker, "deferred stamp work should be visible before delivery starts");

    let progress = daemon
        .handle_rpc(RpcRequest {
            id: 242,
            method: "get_outbound_progress".into(),
            params: Some(json!({ "message_id": "deferred-stamp-queue-1" })),
        })
        .expect("progress")
        .result
        .expect("progress result");
    assert_eq!(progress["progress"].as_f64(), Some(0.0));

    let cancel = daemon
        .handle_rpc(RpcRequest {
            id: 243,
            method: "sdk_cancel_message_v2".into(),
            params: Some(json!({ "message_id": "deferred-stamp-queue-1" })),
        })
        .expect("cancel");
    assert!(cancel.error.is_none(), "cancel should be accepted while stamp is active");

    let mut cancelled = false;
    for _ in 0..80 {
        let status = daemon
            .handle_rpc(RpcRequest {
                id: 244,
                method: "sdk_status_v2".into(),
                params: Some(json!({ "message_id": "deferred-stamp-queue-1" })),
            })
            .expect("status")
            .result
            .expect("status result");
        let lxmf = &status["message"]["fields"]["_lxmf"];
        if status["message"]["receipt_status"] == json!("cancelled")
            && lxmf["stamp_state"] == json!("cancelled")
        {
            let pipeline = &status["delivery_pipeline"];
            let delivery_drained = pipeline["queued_total"].as_u64().unwrap_or_default() == 0;
            let stamp_drained = pipeline["stamp_queued_total"].as_u64().unwrap_or_default() == 0
                && pipeline["stamp_in_flight_total"].as_u64().unwrap_or_default() == 0;
            if delivery_drained && stamp_drained {
                cancelled = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(cancelled, "deferred stamp cancellation should terminalize the queued work");
}
