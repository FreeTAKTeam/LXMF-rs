use super::*;
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn request_boundaries_match_pinned_router_without_pruning_pending_reads() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../libs/test-support/fixtures/lxmf_1_2_0_timing.json"
    )))
    .expect("fixture");
    let destination = AddressHash::new([3; 16]);
    let mut checked = 0;
    for case in fixture["cases"].as_array().expect("cases") {
        let timing = DeliveryTiming::new(
            case["medium_timeout"].as_f64().expect("medium"),
            case["lowest_bitrate"].as_u64(),
            case["first_hop_timeout"].as_f64(),
            case["default_per_hop_timeout"].as_f64().expect("default"),
        )
        .expect("timing");
        for request in case["requests"].as_array().expect("request boundaries") {
            let requests = DeliveryPathRequests::default();
            assert!(requests.claim(destination, timing).expect("initial request"));
            tokio::time::advance(Duration::from_secs_f64(
                request["elapsed"].as_f64().expect("elapsed"),
            ))
            .await;
            for _ in 0..3 {
                assert_eq!(
                    requests.pending(destination, timing).expect("pending"),
                    request["pending"].as_bool().expect("expected pending"),
                    "{}: {request}",
                    case["name"]
                );
            }
            assert_eq!(
                requests.claim(destination, timing).expect("claim"),
                request["allowed"].as_bool().expect("expected allowed"),
                "{}: {request}",
                case["name"]
            );
            checked += 1;
        }
    }
    assert!(checked >= 45);
}

#[tokio::test(start_paused = true)]
async fn suppression_does_not_refresh_or_prune_and_new_claim_uses_current_window() {
    let requests = DeliveryPathRequests::default();
    let a = AddressHash::new([1; 16]);
    let b = AddressHash::new([2; 16]);
    let c = AddressHash::new([3; 16]);
    let slow = DeliveryTiming::from_round_trip(8.0, Some(1999), 8.0).expect("slow");
    let fast = DeliveryTiming::from_round_trip(8.0, Some(2000), 8.0).expect("fast");
    assert!(requests.claim(a, slow).expect("a"));
    tokio::time::advance(Duration::from_secs(10)).await;
    assert!(requests.claim(b, slow).expect("b"));
    tokio::time::advance(Duration::from_secs(5)).await;
    assert!(!requests.claim(b, fast).expect("suppressed"));
    assert!(requests.times.lock().expect("history").contains_key(&a), "suppression cannot prune");
    tokio::time::advance(Duration::from_secs(3)).await;
    assert!(requests.claim(b, fast).expect("original eight-second boundary"));
    assert!(
        !requests.times.lock().expect("history").contains_key(&a),
        "admission prunes with current eight-second window"
    );
    assert!(requests.claim(c, slow).expect("different destination"));
}
