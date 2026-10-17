use lxmf_core::message::{DeliverySchedule, DeliveryTiming};
use std::time::Duration;

fn seconds(value: &serde_json::Value) -> Duration {
    Duration::from_secs_f64(value.as_f64().expect("seconds"))
}

#[test]
fn timing_and_announce_wakeups_match_lxmf_1_2_0() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../test-support/fixtures/lxmf_1_2_0_timing.json"))
            .expect("fixture");
    for case in fixture["cases"].as_array().expect("cases") {
        let timing = DeliveryTiming::new(
            case["medium_timeout"].as_f64().expect("medium seconds"),
            case["lowest_bitrate"].as_u64(),
            case["first_hop_timeout"].as_f64(),
            case["default_per_hop_timeout"].as_f64().expect("per hop seconds"),
        )
        .expect("valid timing");
        assert_eq!(timing.path_request_wait(), seconds(&case["path_wait"]), "{}", case["name"]);
        assert_eq!(
            timing.request_debounce(),
            seconds(&case["request_debounce"]),
            "{}",
            case["name"]
        );
        assert_eq!(
            timing.destination_round_trip(),
            seconds(&case["destination_round_trip"]),
            "{}",
            case["name"]
        );
        for expected in case["schedules"].as_array().expect("schedules") {
            let mut schedule = DeliverySchedule::default();
            assert!(schedule.due(Duration::ZERO, false));
            if expected["path_request"].as_bool().expect("request kind") {
                schedule.schedule_path(Duration::from_secs(1000), timing);
            } else {
                schedule.schedule_delivery(Duration::from_secs(1000), timing);
            }
            assert_eq!(
                schedule.deadline(),
                Some(seconds(&expected["deadline"])),
                "{}",
                case["name"]
            );
            assert_eq!(
                u64::from(schedule.max_attempts()),
                expected["max_attempts"],
                "{}",
                case["name"]
            );
            assert_eq!(schedule.awaiting_path(), expected["awaiting_path"], "{}", case["name"]);
            for check in expected["checks"].as_array().expect("due checks") {
                assert_eq!(
                    schedule
                        .due(seconds(&check["now"]), check["path_known"].as_bool().expect("path")),
                    check["due"],
                    "{} {check}",
                    case["name"]
                );
            }
        }
    }
}

#[test]
fn only_the_attempt_owner_clears_path_waiting() {
    let timing = DeliveryTiming::new(86.0, Some(100), None, 6.0).expect("valid timing");
    let mut schedule = DeliverySchedule::default();
    schedule.schedule_path(Duration::ZERO, timing);
    schedule.schedule_delivery(Duration::ZERO, timing);
    assert!(schedule.awaiting_path());
    assert!(schedule.due(Duration::ZERO, true));
    schedule.begin_attempt();
    assert!(!schedule.awaiting_path());
    assert!(!schedule.due(Duration::ZERO, true));
}

#[test]
fn invalid_measurements_are_rejected_before_scheduling() {
    for invalid in [f64::NAN, f64::INFINITY, -1.0, f64::MAX] {
        assert!(DeliveryTiming::new(invalid, None, None, 6.0).is_err());
        assert!(DeliveryTiming::new(7.0, None, Some(invalid), 6.0).is_err());
        assert!(DeliveryTiming::new(7.0, None, None, invalid).is_err());
    }
}
