#[test]
fn outbound_lxm_progress_does_not_mark_failed_or_cancelled_as_complete() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, status) in [
        ("failed-outbound-query", "failed: no path"),
        ("cancelled-outbound-query", "cancelled"),
        ("rejected-outbound-query", "rejected"),
    ] {
        daemon
            .accept_inbound(MessageRecord {
                id: message_id.to_string(),
                source: "src".to_string(),
                destination: "dst".to_string(),
                title: "title".to_string(),
                content: "content".to_string(),
                timestamp: 1_700_000_000,
                direction: "out".to_string(),
                fields: Some(json!({
                    "_lxmf": {
                        "lxm_hash": format!("{message_id}-hash")
                    }
                })),
                receipt_status: Some(status.to_string()),
            })
            .expect("store outbound");

        let progress = daemon
            .handle_rpc(rpc_request(
                18,
                "get_outbound_progress",
                json!({ "message_id": message_id }),
            ))
            .expect("progress")
            .result
            .expect("progress result");
        assert_eq!(progress["message_id"], json!(message_id));
        assert_eq!(progress["progress"], JsonValue::Null);
    }
}

#[test]
fn outbound_lxm_progress_does_not_treat_completed_stamp_work_as_delivery_complete() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "stamp-ready-outbound-query".to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({
                "_lxmf": {
                    "stamp_state": "ready",
                    "stamp_target_cost": 7,
                    "propagation_stamp_state": "ready",
                    "propagation_stamp_target_cost": 9
                }
            })),
            receipt_status: Some("sending".to_string()),
        })
        .expect("store outbound");

    let progress = daemon
        .handle_rpc(rpc_request(
            19,
            "get_outbound_progress",
            json!({ "message_id": "stamp-ready-outbound-query" }),
        ))
        .expect("progress")
        .result
        .expect("progress result");
    assert_eq!(progress["message_id"], json!("stamp-ready-outbound-query"));
    assert_eq!(progress["progress"].as_f64(), Some(0.01));
}

#[test]
fn outbound_lxm_progress_normalizes_stamp_state_strings() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, lxmf) in [
        ("cancelled-stamp-state-outbound-query", json!({ "stamp_state": " CANCELLED " })),
        (
            "failed-propagation-stamp-state-outbound-query",
            json!({ "propagation_stamp_state": " FAILED " }),
        ),
        ("generating-stamp-state-outbound-query", json!({ "stamp_state": " GENERATING " })),
    ] {
        daemon
            .accept_inbound(MessageRecord {
                id: message_id.to_string(),
                source: "src".to_string(),
                destination: "dst".to_string(),
                title: "title".to_string(),
                content: "content".to_string(),
                timestamp: 1_700_000_000,
                direction: "out".to_string(),
                fields: Some(json!({ "_lxmf": lxmf })),
                receipt_status: Some("sending".to_string()),
            })
            .expect("store outbound");

        let progress = daemon
            .handle_rpc(rpc_request(
                20,
                "get_outbound_progress",
                json!({ "message_id": message_id }),
            ))
            .expect("progress")
            .result
            .expect("progress result");
        if message_id == "generating-stamp-state-outbound-query" {
            assert_eq!(progress["progress"].as_f64(), Some(0.0));
        } else {
            assert_eq!(progress["progress"], JsonValue::Null);
        }
    }
}

#[test]
fn outbound_lxm_progress_ignores_stale_progress_after_terminal_stamp_state() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, lxmf) in [
        (
            "failed-stamp-state-with-stale-progress",
            json!({ "progress": 0.75, "stamp_state": "failed" }),
        ),
        (
            "cancelled-propagation-stamp-state-with-stale-progress",
            json!({ "progress": 0.5, "propagation_stamp_state": "cancelled" }),
        ),
    ] {
        daemon
            .accept_inbound(MessageRecord {
                id: message_id.to_string(),
                source: "src".to_string(),
                destination: "dst".to_string(),
                title: "title".to_string(),
                content: "content".to_string(),
                timestamp: 1_700_000_000,
                direction: "out".to_string(),
                fields: Some(json!({ "_lxmf": lxmf })),
                receipt_status: Some("sending".to_string()),
            })
            .expect("store outbound");

        let progress = daemon
            .handle_rpc(rpc_request(
                21,
                "get_outbound_progress",
                json!({ "message_id": message_id }),
            ))
            .expect("progress")
            .result
            .expect("progress result");

        assert_eq!(progress["message_id"], json!(message_id));
        assert_eq!(progress["progress"], JsonValue::Null);
    }
}

#[test]
fn outbound_lxm_stamp_cost_is_null_when_ticket_stamp_is_used() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "ticket-stamped-outbound-query".to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({
                "_lxmf": {
                    "stamp_kind": "ticket",
                    "stamp_target_cost": 256
                }
            })),
            receipt_status: Some("sending".to_string()),
        })
        .expect("store outbound");

    let stamp_cost = daemon
        .handle_rpc(rpc_request(
            18,
            "get_outbound_lxm_stamp_cost",
            json!({ "message_id": "ticket-stamped-outbound-query" }),
        ))
        .expect("stamp cost")
        .result
        .expect("stamp cost result");
    assert_eq!(stamp_cost["stamp_cost"], JsonValue::Null);
}

#[test]
fn outbound_lxm_stamp_cost_ignores_null_or_empty_ticket_markers() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, lxmf) in [
        (
            "null-ticket-outbound-query",
            json!({
                "stamp_target_cost": 7,
                "outbound_ticket": null,
                "stamp_ticket_source": null
            }),
        ),
        (
            "empty-ticket-outbound-query",
            json!({
                "stamp_target_cost": 8,
                "outbound_ticket": "",
                "stamp_ticket_source": "   "
            }),
        ),
    ] {
        daemon
            .accept_inbound(MessageRecord {
                id: message_id.to_string(),
                source: "src".to_string(),
                destination: "dst".to_string(),
                title: "title".to_string(),
                content: "content".to_string(),
                timestamp: 1_700_000_000,
                direction: "out".to_string(),
                fields: Some(json!({ "_lxmf": lxmf })),
                receipt_status: Some("sending".to_string()),
            })
            .expect("store outbound");

        let stamp_cost = daemon
            .handle_rpc(rpc_request(
                20,
                "get_outbound_lxm_stamp_cost",
                json!({ "message_id": message_id }),
            ))
            .expect("stamp cost")
            .result
            .expect("stamp cost result");
        assert!(stamp_cost["stamp_cost"].as_u64().is_some());
    }
}

#[test]
fn outbound_lxm_stamp_cost_queries_accept_string_cost_metadata() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "string-cost-outbound-query".to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({
                "_lxmf": {
                    "stamp_target_cost": " 7.0 ",
                    "propagation_stamp_target_cost": "9.0"
                }
            })),
            receipt_status: Some("sending".to_string()),
        })
        .expect("store outbound");

    let stamp_cost = daemon
        .handle_rpc(rpc_request(
            21,
            "get_outbound_lxm_stamp_cost",
            json!({ "message_id": "string-cost-outbound-query" }),
        ))
        .expect("stamp cost")
        .result
        .expect("stamp cost result");
    assert_eq!(stamp_cost["stamp_cost"].as_u64(), Some(7));

    let propagation_stamp_cost = daemon
        .handle_rpc(rpc_request(
            22,
            "get_outbound_lxm_propagation_stamp_cost",
            json!({ "message_id": "string-cost-outbound-query" }),
        ))
        .expect("propagation stamp cost")
        .result
        .expect("propagation stamp cost result");
    assert_eq!(propagation_stamp_cost["propagation_stamp_cost"].as_u64(), Some(9));
}

#[test]
fn outbound_lxm_stamp_cost_queries_accept_whole_float_cost_metadata() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "float-cost-outbound-query".to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({
                "_lxmf": {
                    "stamp_target_cost": 7.0,
                    "propagation_stamp_target_cost": 9.0
                }
            })),
            receipt_status: Some("sending".to_string()),
        })
        .expect("store outbound");

    let stamp_cost = daemon
        .handle_rpc(rpc_request(
            23,
            "get_outbound_lxm_stamp_cost",
            json!({ "message_id": "float-cost-outbound-query" }),
        ))
        .expect("stamp cost")
        .result
        .expect("stamp cost result");
    assert_eq!(stamp_cost["stamp_cost"].as_u64(), Some(7));

    let propagation_stamp_cost = daemon
        .handle_rpc(rpc_request(
            24,
            "get_outbound_lxm_propagation_stamp_cost",
            json!({ "message_id": "float-cost-outbound-query" }),
        ))
        .expect("propagation stamp cost")
        .result
        .expect("propagation stamp cost result");
    assert_eq!(propagation_stamp_cost["propagation_stamp_cost"].as_u64(), Some(9));
}

#[test]
fn outbound_lxm_stamp_cost_queries_reject_fractional_cost_metadata() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, stamp_cost_value, propagation_stamp_cost_value) in [
        ("fractional-cost-outbound-query", json!(7.5), json!(9.25)),
        ("negative-float-cost-outbound-query", json!(-7.0), json!(-9.0)),
        ("fractional-string-cost-outbound-query", json!("7.5"), json!("9.25")),
        ("negative-string-cost-outbound-query", json!("-7.0"), json!("-9.0")),
    ] {
        daemon
            .accept_inbound(MessageRecord {
                id: message_id.to_string(),
                source: "src".to_string(),
                destination: "dst".to_string(),
                title: "title".to_string(),
                content: "content".to_string(),
                timestamp: 1_700_000_000,
                direction: "out".to_string(),
                fields: Some(json!({
                    "_lxmf": {
                        "stamp_target_cost": stamp_cost_value,
                        "propagation_stamp_target_cost": propagation_stamp_cost_value
                    }
                })),
                receipt_status: Some("sending".to_string()),
            })
            .expect("store outbound");

        let stamp_cost = daemon
            .handle_rpc(rpc_request(
                25,
                "get_outbound_lxm_stamp_cost",
                json!({ "message_id": message_id }),
            ))
            .expect("stamp cost")
            .result
            .expect("stamp cost result");
        assert_eq!(stamp_cost["stamp_cost"], JsonValue::Null);

        let propagation_stamp_cost = daemon
            .handle_rpc(rpc_request(
                26,
                "get_outbound_lxm_propagation_stamp_cost",
                json!({ "message_id": message_id }),
            ))
            .expect("propagation stamp cost")
            .result
            .expect("propagation stamp cost result");
        assert_eq!(propagation_stamp_cost["propagation_stamp_cost"], JsonValue::Null);
    }
}

#[test]
fn outbound_lxm_stamp_cost_queries_are_null_after_terminal_status() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, status) in [
        ("delivered-cost-query", "delivered"),
        ("sent-cost-query", "sent: direct"),
        ("failed-cost-query", "failed: no path"),
        ("cancelled-cost-query", "cancelled"),
    ] {
        daemon
            .accept_inbound(MessageRecord {
                id: message_id.to_string(),
                source: "src".to_string(),
                destination: "dst".to_string(),
                title: "title".to_string(),
                content: "content".to_string(),
                timestamp: 1_700_000_000,
                direction: "out".to_string(),
                fields: Some(json!({
                    "_lxmf": {
                        "stamp_target_cost": 7,
                        "propagation_stamp_target_cost": 9
                    }
                })),
                receipt_status: Some(status.to_string()),
            })
            .expect("store outbound");

        let stamp_cost = daemon
            .handle_rpc(rpc_request(
                20,
                "get_outbound_lxm_stamp_cost",
                json!({ "message_id": message_id }),
            ))
            .expect("stamp cost")
            .result
            .expect("stamp cost result");
        assert_eq!(stamp_cost["stamp_cost"], JsonValue::Null);

        let propagation_stamp_cost = daemon
            .handle_rpc(rpc_request(
                21,
                "get_outbound_lxm_propagation_stamp_cost",
                json!({ "message_id": message_id }),
            ))
            .expect("propagation stamp cost")
            .result
            .expect("propagation stamp cost result");
        assert_eq!(propagation_stamp_cost["propagation_stamp_cost"], JsonValue::Null);
    }
}
