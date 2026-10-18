use super::*;

fn reference_seconds(row: &serde_json::Value, field: &str) -> Duration {
    Duration::from_secs_f64(f64::from_bits(
        row[format!("{field}_bits")].as_u64().expect("IEEE bits"),
    ))
}

#[test]
fn opportunistic_decisions_match_production_reference_retry_traces() {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../libs/test-support/fixtures/lxmf_1_2_0_retry_traces.json"
    ))
    .expect("retry traces");
    for case in fixtures["cases"].as_array().expect("cases") {
        let mut retry = Retry::default();
        let mut path = case["initial_path_known"].as_bool().expect("path");
        let mut active = true;
        for row in case["rows"].as_array().expect("rows") {
            if let Some(known) = row["event"]["path"].as_bool() {
                path = known;
            }
            let now = reference_seconds(row, "time");
            let timing = DeliveryTiming::new(
                case["medium_timeout"].as_f64().expect("medium"),
                case["lowest_bitrate"].as_u64(),
                path.then(|| row["first_hop"].as_f64().expect("first hop")),
                6.0,
            )
            .expect("timing");
            let action = if row["event"]["admit"] == true && !path {
                retry.schedule.schedule_path(now, timing);
                Action::Request
            } else if active && row["event"]["delivered"] != true && row["event"]["cancel"] != true
            {
                retry.next(now, path, timing)
            } else {
                Action::Wait
            };
            let actions = row["actions"].as_array().expect("actions");
            assert_eq!(
                action == Action::Send,
                actions.iter().any(|a| a == "send"),
                "{} {row}",
                case["name"]
            );
            assert_eq!(retry.attempts as u64, row["attempts"], "{} {row}", case["name"]);
            assert_eq!(
                retry.schedule.max_attempts() as u64,
                row["max_attempts"],
                "{} {row}",
                case["name"]
            );
            assert_eq!(retry.sent_on_path, row["sent_on_path"], "{} {row}", case["name"]);
            assert_eq!(
                retry.schedule.awaiting_path(),
                row["awaiting_path"],
                "{} {row}",
                case["name"]
            );
            if !row["deadline"].is_null() {
                let expected = reference_seconds(row, "deadline");
                let actual = retry.schedule.deadline().expect("deadline");
                // Python adds binary floats; production timers use integer ns.
                // Compare timer representation only within one ns, never relax
                // due decisions, attempt counts or path/terminal transitions.
                assert!(
                    actual.abs_diff(expected) <= Duration::from_nanos(1),
                    "{} {row}: {actual:?}",
                    case["name"]
                );
            }
            if action == Action::Fail {
                assert_eq!(row["state"], 255);
            }
            active = row["pending"].as_bool().expect("pending");
            path = row["path_known"].as_bool().expect("post-action path");
        }
    }
}
