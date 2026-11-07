use super::*;
use rns_rpc::MetricsProvider;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::{IfaceSource, RxMessage};
use rns_transport::packet::{Header, Packet, PacketContext, PacketDataBuffer, PacketType};
use rns_transport::transport::TransportConfig;

#[tokio::test]
async fn final_checkpoint_fences_queued_saves_after_interface_retirement() {
    let root = tempfile::tempdir().expect("checkpoint storage");
    let identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let mut config = TransportConfig::new("final-checkpoint", &identity, true);
    config.set_transport_enabled(true);
    let transport = Arc::new(Transport::new(config));
    let iface = transport.iface_manager().lock().await.new_channel(16);
    let mut remote = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(rand_core::OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let packet = remote.announce(rand_core::OsRng, None).expect("valid announce");
    let destination = packet.destination;
    iface
        .rx_channel
        .send(RxMessage { address: iface.address, packet, source: IfaceSource::None })
        .await
        .expect("announce ingress");
    tokio::time::timeout(Duration::from_secs(2), async {
        while !transport.has_path(&destination).await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("learned route");

    let context = PathTablePersistenceContext::new(transport.clone(), root.path().to_path_buf());
    let trigger = spawn_path_table_persistence_worker_with_interval(
        context.clone(),
        Duration::from_millis(50),
    );
    trigger.send(()).await.expect("queued dirty trigger");
    tokio::time::timeout(Duration::from_secs(2), async {
        while trigger.capacity() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("background worker consumes dirty trigger");
    finish_reticulum_path_table_persistence(Some(context.clone())).await;
    assert_eq!(context.metrics().snapshot()["last_kept_entries"], 1);
    let final_runs = context.metrics().snapshot()["runs_total"].clone();
    let checkpoint =
        tokio::fs::read(root.path().join("destination_table")).await.expect("checkpoint");
    // Native ingress still runs while the early checkpoint persists routes.
    // Confirm a fresh ordinary Proof is accepted/remembered after its hash-list
    // snapshot, before retiring that same live interface.
    let before = transport.table_occupancy().await.packet_cache_hashes;
    let late = Packet {
        header: Header { packet_type: PacketType::Proof, ..Header::default() },
        context: PacketContext::None,
        destination: rns_transport::hash::AddressHash::new_from_slice(&[0x91; 16]),
        data: PacketDataBuffer::new_from_slice(&[0x37; 64]),
        ..Packet::default()
    };
    let late_hash = late.hash();
    let early_hashes = tokio::fs::read(root.path().join("packet_hashlist.raw"))
        .await
        .expect("early replay snapshot");
    assert!(!early_hashes.chunks_exact(32).any(|hash| hash == late_hash.as_slice()));
    iface
        .rx_channel
        .send(RxMessage { address: iface.address, packet: late, source: IfaceSource::None })
        .await
        .expect("late production ingress");
    tokio::time::timeout(Duration::from_secs(2), async {
        while transport.table_occupancy().await.packet_cache_hashes <= before {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("late Proof accepted and remembered");
    assert!(transport.stop_interface(iface.address).await);
    finish_packet_hashlist_persistence(Some(context.clone())).await;
    tokio::time::timeout(Duration::from_secs(2), trigger.closed())
        .await
        .expect("background owner exits after final checkpoint");
    assert_eq!(flush_reticulum_path_table(&context).await.expect("closed background save"), 0);
    maintain_announce_cache(&context).await;
    assert_eq!(
        context.metrics().snapshot()["runs_total"],
        final_runs,
        "final checkpoint is the last save"
    );
    assert_eq!(
        tokio::fs::read(root.path().join("destination_table")).await.expect("retained checkpoint"),
        checkpoint
    );
    assert!(trigger.is_closed(), "background owner exits after final checkpoint");
    let final_hashes = tokio::fs::read(root.path().join("packet_hashlist.raw"))
        .await
        .expect("final replay snapshot");
    assert!(
        final_hashes.chunks_exact(32).any(|hash| hash == late_hash.as_slice()),
        "packet accepted after the early checkpoint must survive final retirement"
    );
    let mut config = TransportConfig::new("replay-replacement", &identity, true);
    config.set_transport_enabled(true);
    let replacement = Transport::new(config);
    assert_eq!(
        replacement.restore_packet_hashlist(root.path()).await.expect("restore late replay hash"),
        final_hashes.len() / 32
    );
    assert_eq!(replacement.table_occupancy().await.packet_cache_hashes, final_hashes.len() / 32);
}
