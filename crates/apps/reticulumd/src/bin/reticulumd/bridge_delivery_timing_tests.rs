use super::*;
use lxmf::message::DeliverySchedule;
use rand_core::OsRng;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::hash::AddressHash;
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::{IfaceSource, RxMessage};
use rns_transport::transport::{NextHopMetrics, Transport, TransportConfig};
use std::time::Duration;

#[test]
fn raw_path_measurements_match_pinned_rns_and_lxmf_arithmetic() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../libs/test-support/fixtures/lxmf_1_2_0_timing_measurements.json"
    )))
    .expect("fixture");
    for case in fixture["cases"].as_array().expect("cases") {
        let measurements = PathTimingSnapshot {
            path_known: case["path_known"].as_bool().expect("path"),
            lowest_interface_bitrate: case["lowest_bitrate"].as_u64(),
            next_hop: case["next_hop_bitrate"].as_u64().map(|bitrate| NextHopMetrics {
                interface: AddressHash::new([7; 16]),
                bitrate,
                // Deliberately not RNS MTU: hardware MTU must not affect timing.
                hardware_mtu: Some(1024),
                per_bit_latency: (bitrate > 0).then(|| 1.0 / bitrate as f64),
            }),
        };
        assert_eq!(
            measurements.medium_timeout_seconds(),
            f64::from_bits(case["medium_timeout_bits"].as_u64().expect("medium bits")),
            "{}",
            case["name"]
        );
        let timing = measured_delivery_timing(measurements).expect("timing");
        assert_eq!(
            timing.path_request_wait(),
            Duration::from_secs_f64(f64::from_bits(
                case["path_wait_bits"].as_u64().expect("wait bits")
            )),
            "{}",
            case["name"]
        );
        assert_eq!(
            timing.destination_round_trip(),
            Duration::from_secs_f64(f64::from_bits(
                case["destination_round_trip_bits"].as_u64().expect("RTT bits")
            )),
            "{}",
            case["name"]
        );
        let mut schedule = DeliverySchedule::default();
        schedule.schedule_delivery(Duration::ZERO, timing);
        assert_eq!(
            u64::from(schedule.max_attempts()),
            case["delivery_attempt_limit"],
            "{}",
            case["name"]
        );
        schedule.schedule_path(Duration::ZERO, timing);
        assert_eq!(
            u64::from(schedule.max_attempts()),
            case["path_attempt_limit"],
            "{}",
            case["name"]
        );
    }
}

#[tokio::test]
async fn timing_snapshot_tracks_learned_path_and_live_interface_bitrates() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("timing-measurements", &identity, true));
    let slow = transport.iface_manager().lock().await.new_channel(16);
    let fast = transport.iface_manager().lock().await.new_channel(16);
    {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        assert!(manager.set_announce_pacing(*slow.address(), 100, 100));
        assert!(manager.set_announce_pacing(*fast.address(), 100000, 100));
    }
    let mut remote = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let destination = remote.desc.address_hash;
    let before = transport.path_timing(&destination).await;
    assert!(!before.path_known);
    assert!(before.next_hop.is_none());
    assert_eq!(before.medium_timeout_seconds(), 86.0);
    fast.rx_channel
        .send(RxMessage {
            address: *fast.address(),
            packet: remote.announce(OsRng, None).expect("announce"),
            source: IfaceSource::None,
        })
        .await
        .expect("ingress");
    tokio::time::timeout(Duration::from_secs(2), async {
        while !transport.has_path(&destination).await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("learned path");
    let known = transport.path_timing(&destination).await;
    assert!(known.path_known);
    assert_eq!(known.next_hop.expect("next hop").interface, *fast.address());
    assert_eq!(known.lowest_interface_bitrate, Some(100));
    assert_eq!(
        measured_delivery_timing(known).expect("timing").destination_round_trip(),
        Duration::from_secs_f64(6.08)
    );
    slow.set_online(false);
    assert_eq!(transport.path_timing(&destination).await.lowest_interface_bitrate, Some(100000));
    slow.set_online(true);
    slow.stop.cancel();
    assert_eq!(transport.path_timing(&destination).await.lowest_interface_bitrate, Some(100000));
    assert!(transport.iface_manager().lock().await.set_announce_pacing(*fast.address(), 0, 100));
    let zero = transport.path_timing(&destination).await;
    assert!(zero.path_known);
    assert_eq!(zero.lowest_interface_bitrate, None);
    assert_eq!(zero.next_hop.expect("zero next hop").bitrate, 0);
    assert_eq!(
        measured_delivery_timing(zero).expect("timing").destination_round_trip(),
        Duration::ZERO
    );
}
