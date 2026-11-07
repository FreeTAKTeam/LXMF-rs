use super::*;
use rns_rpc::MetricsProvider;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::{IfaceSource, RxMessage};
use rns_transport::transport::TransportConfig;

#[tokio::test]
async fn final_checkpoint_fences_queued_saves_after_interface_retirement() {
    let root = tempfile::tempdir().expect("checkpoint storage");
    let identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport =
        Arc::new(Transport::new(TransportConfig::new("final-checkpoint", &identity, true)));
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
    assert!(transport.stop_interface(iface.address).await);
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
}
