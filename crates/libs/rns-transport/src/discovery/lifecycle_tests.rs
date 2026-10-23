use super::*;
use crate::discovery::{DiscoveredInterface, DiscoveryStatus};

include!("tests/autoconnect_reference.rs");

fn discovered(name: &str, host: &str, port: u16) -> DiscoveredInterface {
    DiscoveredInterface {
        discovery_hash: vec![1; 32],
        interface_type: "BackboneInterface".to_string(),
        transport: true,
        impl_name: Some("RNS".to_string()),
        version: Some("1.5.5".to_string()),
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
        operator_lxmf_address: None,
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
fn autoconnect_endpoint_exclusions_apply_even_with_unverified_override() {
    for allow_unverified in [false, true] {
        let policy = AutoconnectInterfacePolicy { allow_unverified, ..Default::default() };
        for endpoint in ["127.0.0.1", "0.0.0.0", "relay.ONION", "200::1", "3ff::1"] {
            assert!(
                plan_autoconnect_with_policy(&discovered("relay", endpoint, 4242), &[], policy)
                    .is_none(),
                "excluded {endpoint}"
            );
        }
        for endpoint in ["127.0.0.2", "::1", "400::1", "relay.example"] {
            assert!(
                plan_autoconnect_with_policy(&discovered("relay", endpoint, 4242), &[], policy)
                    .is_some(),
                "eligible {endpoint}"
            );
        }
    }
}

#[test]
fn autoconnect_allocates_names_against_existing_and_prior_planned_interfaces() {
    let existing = ["relay", "relay (2)"].map(|name| RuntimeInterfaceState {
        id: name.into(),
        autoconnect_hash: None,
        autoconnect_source: None,
        target_host: None,
        target_port: None,
        i2p_b32: None,
        bootstrap_only: false,
        online: false,
        down_since: None,
    });
    let rows = [discovered("relay", "one.example", 4242), discovered("relay", "two.example", 4242)];
    let mut lifecycle = InterfaceDiscoveryLifecycle::default();
    let plans = lifecycle.initial_autoconnect(&rows, &existing, 2);
    assert_eq!(
        plans.iter().map(|plan| plan.name.as_str()).collect::<Vec<_>>(),
        ["relay (3)", "relay (4)"]
    );
}

#[test]
fn initial_autoconnect_override_is_explicit_and_does_not_widen_supported_types() {
    let mut info = discovered("honest-rust", "relay.example", 4242);
    info.impl_name = Some("reticulumd".into());
    info.version = Some("0.14.0".into());
    let mut lifecycle = InterfaceDiscoveryLifecycle::default();
    assert!(lifecycle.initial_autoconnect(std::slice::from_ref(&info), &[], 1).is_empty());
    let policy = AutoconnectInterfacePolicy { allow_unverified: true, ..Default::default() };
    assert_eq!(
        lifecycle
            .initial_autoconnect_with_policy(std::slice::from_ref(&info), &[], 1, policy)
            .len(),
        1
    );
    info.interface_type = "TCPServerInterface".into();
    assert!(lifecycle.initial_autoconnect_with_policy(&[info], &[], 1, policy).is_empty());
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
            allow_unverified: false,
        },
    )
    .expect("eligible discovered interface");
    assert_eq!(plan.interface_mode.as_deref(), Some("gateway"));
    assert_eq!(plan.gravity, 7);
    assert!(plan.announces_to_internal);
}

#[test]
fn rns_1_5_5_autoconnect_requires_verified_backbone_unless_overridden() {
    let mut info = discovered("relay", "relay.example", 4242);
    assert!(plan_autoconnect(&info, &[]).is_some());

    info.impl_name = None;
    assert!(plan_autoconnect(&info, &[]).is_none());
    info.impl_name = Some("other-stack".to_string());
    assert!(plan_autoconnect(&info, &[]).is_none());
    info.impl_name = Some("RNS".to_string());
    info.version = Some("1.5.1".to_string());
    assert!(plan_autoconnect(&info, &[]).is_none());
    info.version = Some("1.5.2".to_string());
    assert!(plan_autoconnect(&info, &[]).is_some());

    info.interface_type = "TCPServerInterface".to_string();
    assert!(plan_autoconnect(&info, &[]).is_none());
    info.interface_type = "BackboneInterface".to_string();
    info.impl_name = None;
    let override_policy = AutoconnectInterfacePolicy {
        allow_unverified: true,
        ..AutoconnectInterfacePolicy::default()
    };
    assert!(plan_autoconnect_with_policy(&info, &[], override_policy).is_some());
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
    assert_eq!(scheduler.next_due(&candidates, 100.0).map(|row| row.id.as_str()), Some("oldest"));
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
