#[test]
fn sdk_conformance_delivery_modes_and_paper_workflows_are_compatible() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    for mode in ["direct", "opportunistic", "propagated"] {
        let message_id = format!("mode-{mode}-{}", timestamp_millis());
        let mut send_params = json!({
            "id": message_id,
            "source": "source.test",
            "destination": "destination.test",
            "title": "",
            "content": format!("content-{mode}"),
            "method": mode
        });
        if mode == "propagated" {
            send_params["include_ticket"] = json!(true);
            send_params["try_propagation_on_fail"] = json!(true);
            send_params["stamp_cost"] = json!(8);
        }

        let send_response = harness.rpc_call("send_message_v2", Some(send_params));
        assert!(send_response.error.is_none(), "send_message_v2 should succeed for mode={mode}");

        let trace_response =
            harness.rpc_call("message_delivery_trace", Some(json!({ "message_id": message_id })));
        assert!(
            trace_response.error.is_none(),
            "message_delivery_trace should succeed for mode={mode}"
        );
        let statuses = trace_response
            .result
            .and_then(|value| value.get("transitions").cloned())
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|transition| {
                transition.get("status").and_then(JsonValue::as_str).map(str::to_owned)
            })
            .collect::<Vec<_>>();
        assert!(
            statuses.iter().any(|status| status.contains(&format!("sent: {mode}"))),
            "delivery trace should contain sent status for mode={mode}; statuses={statuses:?}"
        );
    }

    let paper_message_id = format!("paper-msg-{}", timestamp_millis());
    let paper_send = harness.rpc_call(
        "send_message_v2",
        Some(json!({
            "id": paper_message_id,
            "source": "source.test",
            "destination": "destination.test",
            "title": "",
            "content": "paper workflow body"
        })),
    );
    assert!(paper_send.error.is_none(), "send_message_v2 should succeed for paper workflow");

    let paper_encode =
        harness.rpc_call("sdk_paper_encode_v2", Some(json!({ "message_id": paper_message_id })));
    assert!(paper_encode.error.is_none(), "sdk_paper_encode_v2 should succeed");
    let uri = paper_encode
        .result
        .and_then(|value| value.get("envelope").cloned())
        .and_then(|value| value.get("uri").cloned())
        .and_then(|value| value.as_str().map(str::to_owned))
        .expect("paper encode response must include envelope uri");

    let paper_decode = harness.rpc_call("sdk_paper_decode_v2", Some(json!({ "uri": uri })));
    assert!(paper_decode.error.is_none(), "sdk_paper_decode_v2 should succeed");
    let paper_result = paper_decode.result.expect("paper decode result");
    assert_eq!(paper_result["accepted"], json!(true));
    assert_eq!(paper_result["destination"], json!("destination.test"));
    assert!(
        paper_result["transient_id"].as_str().is_some_and(|value| !value.is_empty()),
        "paper ingest must return a transient ID"
    );
    assert_eq!(paper_result["duplicate"], json!(false));
    assert_eq!(
        paper_result["bytes_len"].as_u64(),
        Some(u64::try_from(uri.len()).expect("URI length fits u64"))
    );

    let duplicate_decode = harness.rpc_call("sdk_paper_decode_v2", Some(json!({ "uri": uri })));
    assert!(duplicate_decode.error.is_none(), "duplicate paper ingest should succeed");
    let duplicate_result = duplicate_decode.result.expect("duplicate paper decode result");
    assert_eq!(duplicate_result["transient_id"], paper_result["transient_id"]);
    assert_eq!(duplicate_result["destination"], paper_result["destination"]);
    assert_eq!(duplicate_result["bytes_len"], paper_result["bytes_len"]);
    assert_eq!(duplicate_result["duplicate"], json!(true));
}
