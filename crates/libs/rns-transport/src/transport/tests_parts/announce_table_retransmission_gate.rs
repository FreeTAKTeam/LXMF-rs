use super::announce::announce_retransmit_tick;

async fn feed_announce(transport: &Transport, iface: crate::hash::AddressHash, aspect: &str) -> Packet {
    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", aspect),
    );
    let announce = destination.announce(OsRng, None).expect("announce");
    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    announce
}

async fn tier_sizes(transport: &Transport) -> (usize, usize) {
    transport.get_handler().lock().await.announce_table.tier_sizes()
}

// Pinned Python: 1.0 s announce check + one 0.25 s jobs-loop poll. The
// ignored source-contract test below verifies both inputs against the pin.
const PYTHON_LOCAL_ANNOUNCE_POLL_BOUND: Duration = Duration::from_millis(1_250);

/// A node that will never retransmit must not accumulate a retransmission
/// queue. `map` is pruned only by `drain_retransmissions`, and the retransmit
/// worker calls that only when `transport_enabled` — so before this gate,
/// every announce a passive node heard stayed in `map` for the life of the
/// process, one cloned `Packet` per distinct destination.
///
/// The reference does not have the problem because it never inserts:
/// `Transport.py`'s `if (transport_enabled() or is_from_local_client) and
/// context != PATH_RESPONSE:` guards the insert itself.
#[tokio::test]
async fn passive_transport_caches_announces_instead_of_queueing_them() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("passive", &identity, false));
    let iface = transport.iface_manager().lock().await.new_channel(16).address;

    for aspect in ["one", "two", "three"] {
        feed_announce(&transport, iface, aspect).await;
    }

    let (queued, cached) = tier_sizes(&transport).await;
    assert_eq!(queued, 0, "a passive node must not queue announces for a retransmission it will never send");
    assert_eq!(cached, 3, "they belong in the bounded cache instead, which is what the path table reads back");
}

/// The control. Same three announces, same code path, `transport_enabled` on:
/// they are queued, because this node really will rebroadcast them. Without
/// this, the assertion above would also pass if `handle_announce` had simply
/// stopped working.
#[tokio::test]
async fn transport_enabled_still_queues_announces_for_retransmission() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("relay", &identity, false);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let iface = transport.iface_manager().lock().await.new_channel(16).address;

    for aspect in ["one", "two", "three"] {
        feed_announce(&transport, iface, aspect).await;
    }

    let (queued, cached) = tier_sizes(&transport).await;
    assert_eq!(queued, 3, "a transport node must still queue what it is going to rebroadcast");
    assert_eq!(cached, 0, "and must not divert them to the cache");
}

/// Reference parity for the clause this crate models from the other side: an
/// announce arriving over a shared-instance link is queued even on a passive
/// node, matching `or is_from_local_client`.
#[tokio::test]
async fn a_shared_instance_iface_still_queues_on_a_passive_node() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("passive-shared", &identity, false));
    let iface = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let parent = *manager.new_channel(16).address();
        assert!(manager.set_shared_instance(parent, true));
        manager
            .register_virtual_iface(parent, crate::iface::IfaceRole::Unicast)
            .expect("local client iface")
    };

    feed_announce(&transport, iface, "local-client").await;

    let (queued, cached) = tier_sizes(&transport).await;
    assert_eq!(queued, 1, "the reference queues a local client's announce even when not transport-enabled");
    assert_eq!(cached, 0);
}

#[tokio::test]
async fn local_client_announce_retransmits_on_first_worker_tick_once() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("passive-worker-tick", &identity, false));
    let (mut host_channel, local_client) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let host_channel = manager.new_channel(16);
        let parent = *host_channel.address();
        assert!(manager.set_shared_instance(parent, true));
        let local_client = manager
            .register_virtual_iface(parent, crate::iface::IfaceRole::Unicast)
            .expect("local client iface");
        (host_channel, local_client)
    };

    let announce = feed_announce(&transport, local_client, "local-client-worker-tick").await;
    let handler = transport.get_handler();
    let due = handler
        .lock()
        .await
        .announce_table
        .timeout_for_destination(&announce.destination)
        .expect("local-client announce is queued immediately");
    let prior_tick = due - Duration::from_nanos(1);
    let first_tick_after_due = prior_tick + INTERVAL_ANNOUNCES_RETRANSMIT;

    assert!(prior_tick < due);
    assert!(first_tick_after_due > due);
    assert!(
        first_tick_after_due - due < INTERVAL_ANNOUNCES_RETRANSMIT,
        "the first worker tick after an immediate deadline is within one worker interval"
    );
    assert!(
        first_tick_after_due - due < PYTHON_LOCAL_ANNOUNCE_POLL_BOUND,
        "the deterministic Rust tick is strictly inside Python's source-derived polling bound"
    );

    announce_retransmit_tick(&handler, prior_tick).await;
    assert!(
        host_channel.tx_channel.try_recv().is_err(),
        "no local-client rebroadcast is sent before its deadline"
    );
    assert_eq!(tier_sizes(&transport).await, (1, 0));

    announce_retransmit_tick(&handler, first_tick_after_due).await;
    let message = host_channel
        .tx_channel
        .try_recv()
        .expect("the first worker tick after the deadline emits the local-client rebroadcast");
    assert!(matches!(
        message.tx_type,
        TxMessageType::Broadcast(Some(iface)) if iface == local_client
    ));
    assert_eq!(message.packet.destination, announce.destination);
    assert_eq!(message.packet.data, announce.data);
    assert_eq!(message.packet.transport, Some(*identity.address_hash()));
    assert_eq!(message.packet.header.propagation_type, crate::packet::PropagationType::Transport);
    assert_eq!(tier_sizes(&transport).await, (0, 1));

    announce_retransmit_tick(
        &handler,
        first_tick_after_due + INTERVAL_ANNOUNCES_RETRANSMIT,
    )
    .await;
    assert!(
        host_channel.tx_channel.try_recv().is_err(),
        "the one local-client retry is not emitted again on a later worker tick"
    );
    assert_eq!(tier_sizes(&transport).await, (0, 1));
}

#[test]
#[ignore = "requires pinned Python Reticulum checkout at RETICULUM_PY_REPO"]
fn pinned_python_local_client_schedule_bounds_the_rust_worker_tick() {
    const PINNED_RETICULUM: &str = "99de23c040d507e3fefca19e87b182302902725d";
    let python_repo = std::env::var("RETICULUM_PY_REPO")
        .expect("set RETICULUM_PY_REPO to the pinned Python Reticulum checkout");
    let revision = std::process::Command::new("git")
        .args(["-C", &python_repo, "rev-parse", "HEAD"])
        .output()
        .expect("read pinned Python Reticulum revision");
    assert!(revision.status.success(), "git rev-parse failed for {python_repo}");
    assert_eq!(
        String::from_utf8_lossy(&revision.stdout).trim(),
        PINNED_RETICULUM,
        "schedule contract must be read from the issue's pinned Python reference"
    );

    let script = r#"
import ast
import inspect
from RNS.Transport import Transport

tree = ast.parse(inspect.getsource(Transport))
assert any(
    isinstance(node, ast.If)
    and "transport_enabled" in ast.unparse(node.test)
    and "is_from_local_client" in ast.unparse(node.test)
    for node in ast.walk(tree)
), "local-client announces must be queued even when transport forwarding is disabled"
local_client_due_entry = False
for node in ast.walk(tree):
    if isinstance(node, ast.If) and isinstance(node.test, ast.Name) and node.test.id == "is_from_local_client":
        assignments = [ast.unparse(child) for child in ast.walk(node) if isinstance(child, ast.Assign)]
        if "retransmit_timeout = now" in assignments and "retries = Transport.PATHFINDER_R" in assignments:
            local_client_due_entry = True
assert local_client_due_entry, "local-client announce must be due at now with PATHFINDER_R retries"
assert Transport.PATHFINDER_R == 1
assert Transport.announces_check_interval == 1.0
assert Transport.job_interval == 0.25
jobs = inspect.getsource(Transport.jobs)
assert "time.time() > announce_entry[IDX_AT_RTRNS_TMO]" in jobs
assert "announce_entry[IDX_AT_RETRIES] > Transport.PATHFINDER_R" in jobs
print(f"{Transport.PATHFINDER_R},{int(Transport.announces_check_interval * 1000)},{int(Transport.job_interval * 1000)}")
"#;
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let output = std::process::Command::new(python)
        .args(["-c", script])
        .env(
            "PYTHONPATH",
            format!("{python_repo}:{}", std::env::var("PYTHONPATH").unwrap_or_default()),
        )
        .output()
        .expect("run pinned Python local-client schedule contract");
    assert!(
        output.status.success(),
        "pinned Python schedule contract failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let constants = String::from_utf8_lossy(&output.stdout);
    let values: Vec<u64> = constants
        .trim()
        .split(',')
        .map(|value| value.parse().expect("Python schedule value is an integer"))
        .collect();
    assert_eq!(values, [1, 1_000, 250]);
    let python_poll_bound = Duration::from_millis(values[1] + values[2]);
    assert_eq!(python_poll_bound, PYTHON_LOCAL_ANNOUNCE_POLL_BOUND);
    assert!(
        INTERVAL_ANNOUNCES_RETRANSMIT < python_poll_bound,
        "Rust's first 1.0 s worker tick is inside Python's source-derived 1.0 s check + 0.25 s poll bound"
    );
}

#[tokio::test]
async fn accepted_announce_fans_out_directly_to_other_local_clients() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("local-client-fanout", &identity, false));
    let (mut host_channel, local_client, other_local_client) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let host_channel = manager.new_channel(16);
        let parent = *host_channel.address();
        assert!(manager.set_shared_instance(parent, true));
        let local_client = manager
            .register_virtual_iface(parent, crate::iface::IfaceRole::Unicast)
            .expect("first local client iface");
        let other_local_client = manager
            .register_virtual_iface(parent, crate::iface::IfaceRole::Unicast)
            .expect("second local client iface");
        (host_channel, local_client, other_local_client)
    };

    let announce = feed_announce(&transport, local_client, "local-client-fanout").await;
    let message = timeout(Duration::from_millis(250), host_channel.tx_channel.recv())
        .await
        .expect("local-client fanout should be immediate")
        .expect("host transmit queue remains open");

    assert!(matches!(
        message.tx_type,
        crate::iface::TxMessageType::Direct(iface) if iface == other_local_client
    ));
    assert_eq!(message.packet.destination, announce.destination);
    assert_eq!(message.packet.data, announce.data);
    assert_eq!(message.packet.context, PacketContext::None);
    assert_eq!(message.packet.transport, Some(*identity.address_hash()));
    assert_eq!(message.packet.header.header_type, crate::packet::HeaderType::Type2);
    assert_eq!(
        message.packet.header.propagation_type,
        crate::packet::PropagationType::Transport
    );
    assert_eq!(message.packet.header.hops, announce.header.hops);
    assert!(timeout(Duration::from_millis(25), host_channel.tx_channel.recv()).await.is_err());
}

/// A locally hosted destination is not a remote route, even when its valid
/// announce re-enters through one shared-instance child. It must not be
/// retransmitted or fanned out to sibling local clients.
#[tokio::test]
async fn locally_hosted_announce_is_not_learned_or_fanned_out() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("local-destination-no-transit", &identity, false);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let (mut host_channel, local_client, other_local_client) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let host_channel = manager.new_channel(16);
        let parent = *host_channel.address();
        assert!(manager.set_shared_instance(parent, true));
        let local_client = manager
            .register_virtual_iface(parent, crate::iface::IfaceRole::Unicast)
            .expect("first local client iface");
        let other_local_client = manager
            .register_virtual_iface(parent, crate::iface::IfaceRole::Unicast)
            .expect("second local client iface");
        (host_channel, local_client, other_local_client)
    };

    let destination = transport
        .add_destination(identity, DestinationName::new("lxmf", "locally-hosted"))
        .await;
    let announce = destination.lock().await.announce(OsRng, None).expect("local announce");
    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        local_client,
        crate::iface::IfaceSource::None,
    )
    .await;

    let handler = transport.get_handler();
    let handler = handler.lock().await;
    assert!(
        handler.path_table.get(&announce.destination).is_none(),
        "a local destination must not acquire a remote route from its own announce"
    );
    assert_eq!(handler.announce_table.tier_sizes(), (0, 0));
    drop(handler);
    assert!(
        timeout(Duration::from_millis(25), host_channel.tx_channel.recv()).await.is_err(),
        "a local destination announce must not transit to sibling clients"
    );
    assert_ne!(local_client, other_local_client);
}

/// A path response is a directed reply, not something to rebroadcast, so it is
/// cached rather than queued even on a transport node.
#[tokio::test]
async fn a_path_response_announce_is_never_queued_for_retransmission() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("relay-path-response", &identity, false);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let iface = transport.iface_manager().lock().await.new_channel(16).address;

    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "path-response"),
    );
    let mut announce = destination.announce(OsRng, None).expect("announce");
    announce.context = PacketContext::PathResponse;
    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let (queued, cached) = tier_sizes(&transport).await;
    assert_eq!(queued, 0, "a path response is a directed reply, not a rebroadcast candidate");
    assert_eq!(cached, 1);
}

/// The reason nothing is simply dropped. This crate rebuilds a path entry's
/// announce packet out of the announce table when persisting the path table,
/// where the reference stores a packet hash and keeps the packet in its own
/// on-disk cache. A passive node that dropped the packet here would persist an
/// empty path table — so the cached copy has to remain findable.
#[tokio::test]
async fn a_cached_announce_is_still_findable_for_path_table_persistence() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("passive-persist", &identity, false));
    let iface = transport.iface_manager().lock().await.new_channel(16).address;

    let announce = feed_announce(&transport, iface, "persisted").await;

    let handler_arc = transport.get_handler();
    let handler = handler_arc.lock().await;
    let found = handler.announce_table.cached_packet_for_destination(&announce.destination);
    assert!(
        found.is_some(),
        "save_reticulum_path_table drops any path entry whose announce packet it cannot find"
    );
}

#[tokio::test]
async fn newer_cached_path_announce_survives_scheduled_queue_restart() {
    let temp = tempfile::tempdir().expect("path-table storage");
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("announce-persistence", &identity, false);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "cached-scheduled-restart"),
    );

    let scheduled = destination.announce(OsRng, None).expect("scheduled announce");
    handle_announce(
        &scheduled,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let mut cached = destination.announce(OsRng, None).expect("cached announce");
    cached.context = PacketContext::PathResponse;
    handle_announce(
        &cached,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert_eq!(tier_sizes(&transport).await, (1, 0));
    let destination_hash = cached.destination;
    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);

    let mut restored_config = TransportConfig::new("announce-persistence", &identity, false);
    restored_config.set_transport_enabled(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restored_iface, iface, "test relies on deterministic interface hashes");
    let report = restored
        .restore_reticulum_path_table_report(temp.path())
        .await
        .expect("restore");

    assert_eq!(report.restored_active_paths, 1);
    assert!(restored.has_path(&destination_hash).await);
    assert_eq!(tier_sizes(&restored).await, (0, 1));
    let handler = restored.get_handler();
    let handler = handler.lock().await;
    let persisted = handler
        .announce_table
        .cached_packet_for_destination(&destination_hash)
        .expect("restored announce cache entry");
    assert_eq!(persisted.data, cached.data, "restart must retain the latest accepted announce");
    assert_eq!(
        persisted.context,
        PacketContext::None,
        "a restored cache entry must not become scheduled retransmission work"
    );
}
