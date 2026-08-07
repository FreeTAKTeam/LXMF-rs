use super::*;
use crate::discovery::{DiscoveredInterface, DiscoveryStatus};

fn discovered(name: &str, host: &str, port: u16) -> DiscoveredInterface {
    DiscoveredInterface {
        discovery_hash: vec![1; 32],
        interface_type: "BackboneInterface".to_string(),
        transport: true,
        name: name.to_string(),
        received: 1.0,
        stamp: vec![2; 32],
        value: 14,
        transport_id: "11".repeat(16),
        network_id: "22".repeat(16),
        hops: 1,
        latitude: None,
        longitude: None,
        height: None,
        reachable_on: Some(host.to_string()),
        port: Some(port),
        ifac_netname: None,
        ifac_netkey: None,
        config_entry: None,
        discovered: 1.0,
        last_heard: 1.0,
        heard_count: 0,
        status: DiscoveryStatus::Available,
        status_code: 1000,
    }
}

#[test]
fn initial_autoconnect_obeys_maximum_and_duplicate_endpoint_rules() {
    let rows = [discovered("one", "one.example", 1), discovered("two", "two.example", 2)];
    let mut lifecycle = InterfaceDiscoveryLifecycle::default();
    let plans = lifecycle.initial_autoconnect(&rows, &[], 1);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].target_host, "one.example");
    assert!(lifecycle.initial_autoconnect_ran());
    let existing = [RuntimeInterfaceState {
        id: "existing".to_string(),
        autoconnect_hash: Some(endpoint_hash(&rows[0])),
        autoconnect_source: None,
        target_host: None,
        target_port: None,
        i2p_b32: None,
        bootstrap_only: false,
        online: true,
        down_since: None,
    }];
    assert!(plan_autoconnect(&rows[0], &existing).is_none());
}

#[test]
fn autoconnect_policy_carries_mode_gravity_and_internal_announce_flag() {
    let info = discovered("policy", "policy.example", 4242);
    let plan = plan_autoconnect_with_policy(
        &info,
        &[],
        AutoconnectInterfacePolicy {
            mode: Some("gateway"),
            gravity: 7,
            announces_to_internal: true,
        },
    )
    .expect("eligible discovered interface");
    assert_eq!(plan.interface_mode.as_deref(), Some("gateway"));
    assert_eq!(plan.gravity, 7);
    assert!(plan.announces_to_internal);
}

#[test]
fn announce_scheduler_selects_most_overdue_discoverable_interface() {
    let mut scheduler = InterfaceAnnounceScheduler::default();
    let candidates = [
        DiscoveryAnnouncementCandidate {
            id: "recent".to_string(),
            supports_discovery: true,
            discoverable: true,
            last_announce: 80.0,
            announce_interval: 10.0,
        },
        DiscoveryAnnouncementCandidate {
            id: "oldest".to_string(),
            supports_discovery: true,
            discoverable: true,
            last_announce: 10.0,
            announce_interval: 10.0,
        },
    ];
    assert!(scheduler.next_due(&candidates, 100.0).is_none());
    scheduler.start();
    assert_eq!(
        scheduler.next_due(&candidates, 100.0).map(|row| row.id.as_str()),
        Some("oldest")
    );
    scheduler.stop();
    assert!(scheduler.next_due(&candidates, 100.0).is_none());
}

#[test]
fn monitor_marks_then_detaches_and_reenables_bootstrap() {
    let mut lifecycle = InterfaceDiscoveryLifecycle::default();
    lifecycle.monitor_interface("auto");
    let mut interfaces = [RuntimeInterfaceState {
        id: "auto".to_string(),
        autoconnect_hash: Some([1; 32]),
        autoconnect_source: None,
        target_host: None,
        target_port: None,
        i2p_b32: None,
        bootstrap_only: false,
        online: false,
        down_since: None,
    }];
    let first = lifecycle.monitor(10, &mut interfaces, 4);
    assert_eq!(first.marked_down, ["auto"]);
    assert!(first.enable_bootstrap);
    let detached = lifecycle.monitor(22, &mut interfaces, 4);
    assert_eq!(detached.detach, ["auto"]);
}

#[test]
fn blackhole_scheduler_waits_and_merges_without_overwrite() {
    let mut scheduler = BlackholeUpdateScheduler::default();
    scheduler.start(100);
    let sources = vec!["source".to_string()];
    assert!(scheduler.due_sources(&sources, 119).is_empty());
    assert_eq!(scheduler.due_sources(&sources, 3_601), sources);
    scheduler.mark_updated("source", 3_601);
    assert!(scheduler.due_sources(&sources, 3_602).is_empty());

    let original = BlackholeEntry { source: "local".to_string(), until: None, reason: None };
    let mut current = BTreeMap::from([("known".to_string(), original.clone())]);
    let update = BTreeMap::from([
        (
            "known".to_string(),
            BlackholeEntry { source: "remote".to_string(), until: None, reason: None },
        ),
        (
            "new".to_string(),
            BlackholeEntry {
                source: "remote".to_string(),
                until: Some(10),
                reason: Some("test".to_string()),
            },
        ),
    ]);
    assert_eq!(merge_blackhole_update(&mut current, update), 1);
    assert_eq!(current["known"], original);
    assert!(current.contains_key("new"));
}
