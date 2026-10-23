use super::*;
use crate::hash::AddressHash;
use crate::iface::{IfaceSource, InterfaceManager, RxMessage};
use crate::packet::{Packet, PacketDataBuffer};
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time::{sleep, timeout, Duration};

async fn blocked_receive_shutdown(full_events: bool) {
    let wait = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other).initial_peering_wait;
    assert_eq!(wait, Duration::from_millis(1920));
    let mut plan = loopback_software_auto_plan(wait);
    plan.candidates.clear();
    plan.adopted_devices.clear();
    plan.peering_packets.clear();
    plan.device_filter.allowed = vec!["lo".into()];
    let peer = tokio::net::UdpSocket::bind("127.0.0.3:0").await.expect("owned loopback peer");
    let peer_address = peer.local_addr().expect("peer endpoint");
    plan.config.data_port = peer_address.port();
    let mut sockets =
        plan.bind_data_sockets_with_native_scope_ids().await.expect("native data socket");
    assert_eq!(sockets.len(), 1);
    let socket = sockets.pop().expect("one owned native socket");
    let bound = socket.bind_addr;
    let started_at = Instant::now();
    sleep(wait).await;
    assert!(started_at.elapsed() >= wait);
    let mut state = plan.discovery_state();
    let admitted = plan
        .process_discovery_datagram(
            &mut state,
            AutoDiscoveryDatagram {
                kind: AutoDiscoverySocketKind::Unicast,
                ifname: "lo".into(),
                bind_addr: bound,
                multicast_group_addr: None,
                source_addr: peer_address,
                payload: crate::iface::auto::peering_token(
                    plan.config.group_id.as_bytes(),
                    &peer_address.ip().to_string(),
                )
                .to_vec(),
            },
            started_at.elapsed(),
        )
        .expect("valid group token")
        .expect("one native peer after protocol gate");
    assert!(matches!(admitted.event, AutoDiscoveryEvent::Peer(_)));

    let mut manager = InterfaceManager::new(1);
    let channel = manager.new_channel_with_role(8, IfaceRole::Multicast);
    let parent = channel.address;
    let rx_sender = channel.rx_channel.clone();
    let rx_messages = manager.receiver();
    let sentinel = RxMessage {
        address: parent,
        source: IfaceSource::None,
        packet: Packet {
            data: PacketDataBuffer::new_from_slice(b"held-native-input"),
            ..Default::default()
        },
    };
    if !full_events {
        rx_sender.send(sentinel.clone()).await.expect("fill actual native receive queue");
        assert_eq!(rx_sender.capacity(), 0);
    }
    let manager = Arc::new(Mutex::new(manager));
    let (bridge, _tx_channel) =
        AutoInterfaceTransportRuntime::from_channel(channel, manager.clone()).split();
    let (shutdown, shutdown_rx) = watch::channel(false);
    let (events, mut event_messages) = mpsc::channel(1);
    if full_events {
        events
            .send(AutoPeerDataLoopEvent::ReceiveFailed {
                ifname: "lo".into(),
                bind_addr: bound,
                error: "owned full event queue".into(),
            })
            .await
            .expect("fill actual receive-worker event queue");
        assert_eq!(events.capacity(), 0);
    }
    let status = AutoRuntimeStatusHandle::from_startup_plan(&plan.startup_plan);
    let mut receiver = plan.spawn_peer_data_receive_loop(
        socket,
        AutoPeerDataReceiveLoopRuntime {
            state: Arc::new(Mutex::new(state)),
            dedupe: Arc::new(Mutex::new(AutoInboundPacketDeduplicator::from_timing(
                AutoInterfaceTiming::for_platform(plan.platform),
            ))),
            transport: Some(bridge),
            runtime_status: Some(status.clone()),
            events,
            shutdown: shutdown_rx,
            started_at,
        },
    );
    let packet = Packet {
        destination: AddressHash::new([0xc1; 16]),
        data: PacketDataBuffer::new_from_slice(b"blocked-native-auto-handoff"),
        ..Default::default()
    };
    let bytes = packet.to_bytes().expect("native input packet");
    assert_eq!(Packet::from_bytes(&bytes).expect("valid nonempty native packet").data, packet.data);
    assert_eq!(peer.send_to(&bytes, bound).await.expect("native data input"), bytes.len());
    let child = timeout(Duration::from_secs(3), async {
        loop {
            let child = manager
                .lock()
                .await
                .traffic_snapshots()
                .into_iter()
                .find(|s| s.parent == Some(parent));
            if let Some(child) = child {
                if !full_events || status.to_json()["peer_data_delivered_count"] == 1 {
                    break child.address;
                }
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native blocked-handoff barrier");
    // On this current-thread runtime, forwarding has yielded at the held queue.
    assert_eq!(rx_messages.lock().await.len(), 1);
    assert_eq!(event_messages.len(), usize::from(full_events));
    assert!(!receiver.is_finished());
    shutdown.send_replace(true);
    timeout(Duration::from_secs(2), &mut receiver)
        .await
        .expect("native Auto receive owner must join while its queue remains full")
        .expect("native Auto receive owner joined");
    assert_eq!(rx_messages.lock().await.len(), 1);
    assert_eq!(event_messages.len(), usize::from(full_events));
    let _rebound = std::net::UdpSocket::bind(bound)
        .expect("joined receive owner releases socket before manager retirement");
    assert!(manager.lock().await.interface_hashes().contains(&child));
    let delivered = rx_messages.lock().await.try_recv().expect("exact held native input");
    if full_events {
        assert_eq!(status.to_json()["peer_data_delivered_count"], 1);
        assert_eq!(delivered.address, child);
        assert_eq!(delivered.packet.destination, packet.destination);
        assert_eq!(delivered.packet.data, packet.data);
        assert_eq!(delivered.source, IfaceSource::Udp(peer_address));
        assert!(
            matches!(event_messages.try_recv().expect("original event remains queued"), AutoPeerDataLoopEvent::ReceiveFailed { error, .. } if error == "owned full event queue")
        );
    } else {
        assert_eq!(status.to_json()["peer_data_delivered_count"], 0);
        assert_eq!(delivered, sentinel, "cancelled forwarding never replaces held input");
        assert!(event_messages.try_recv().is_err());
    }
    assert!(rx_messages.lock().await.try_recv().is_err());
    assert!(manager.lock().await.stop_interface(parent));
    assert!(manager.lock().await.interface_hashes().is_empty());
}

#[tokio::test]
async fn native_auto_receiver_shutdown_joins_full_transport_queue() {
    if auto_lifecycle_runs_in_child() {
        return;
    }
    blocked_receive_shutdown(false).await;
}

#[tokio::test]
async fn native_auto_receiver_shutdown_joins_full_event_queue() {
    if auto_lifecycle_runs_in_child() {
        return;
    }
    blocked_receive_shutdown(true).await;
}
