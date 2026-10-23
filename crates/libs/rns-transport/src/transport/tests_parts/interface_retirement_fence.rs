use super::*;
use crate::iface::{IfaceRole, Interface, InterfaceContext};
use std::future::{poll_fn, Future};
use std::task::Poll;

struct Carrier;
impl Interface for Carrier {
    fn mtu() -> usize {
        500
    }
}

#[tokio::test]
async fn retirement_fences_late_children_while_transport_cleanup_waits_for_handler() {
    let transport = Transport::new(TransportConfig::default());
    let manager = transport.iface_manager();
    let constructed = Arc::new(AtomicUsize::new(0));
    let ended = Arc::new(AtomicUsize::new(0));
    let worker = |constructed: Arc<AtomicUsize>, ended: Arc<AtomicUsize>| {
        move |context: InterfaceContext<Carrier>| {
            constructed.fetch_add(1, Ordering::SeqCst);
            async move {
                context.channel.stop.cancelled().await;
                ended.fetch_add(1, Ordering::SeqCst);
            }
        }
    };
    let root = manager.lock().await.spawn_as(
        Carrier,
        worker(constructed.clone(), ended.clone()),
        IfaceRole::Multicast,
    );
    let child = manager
        .lock()
        .await
        .spawn_inheriting(root, Carrier, worker(constructed.clone(), ended.clone()))
        .expect("preexisting accepted child");
    let unrelated = manager.lock().await.new_channel(2);
    let routing = Arc::new(tokio::sync::Mutex::new(crate::iface::udp::PeerRouting::new()));
    transport.handler.lock().await.register_multicast_peer_routing(root, routing.clone());
    let peer = "127.0.0.1:4343".parse().expect("peer address");
    let virtual_child = transport
        .handler
        .lock()
        .await
        .unicast_iface_for_source(root, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("existing virtual peer");
    let handler = transport.handler.lock().await;
    assert!(handler.unicast_udp_ifaces.contains_key(&peer));
    let workers = manager.lock().await.retire_worker_tree(root);
    assert_eq!(workers.len(), 2);
    let mut stopping = Box::pin(transport.stop_interface(root));
    poll_fn(|context| {
        assert!(
            stopping.as_mut().poll(context).is_pending(),
            "transport cleanup is held behind the handler"
        );
        Poll::Ready(())
    })
    .await;
    {
        let mut manager = manager.lock().await;
        assert_eq!(manager.online(&root), Some(false));
        assert_eq!(manager.online(&child), Some(false));
        assert!(
            manager.register_virtual_iface(root, IfaceRole::VirtualUnicast).is_none(),
            "cancelled host cannot publish a late virtual peer"
        );
        assert!(
            manager
                .spawn_inheriting(root, Carrier, worker(constructed.clone(), ended.clone()))
                .is_none(),
            "cancelled parent cannot construct a late accepted worker"
        );
        assert!(
            manager
                .spawn_local_shared_client(
                    root,
                    Carrier,
                    worker(constructed.clone(), ended.clone())
                )
                .is_none(),
            "native shared-client exception does not bypass retirement"
        );
        manager.cleanup();
        assert!(!manager.interface_hashes().contains(&root));
        assert!(!manager.interface_hashes().contains(&virtual_child));
        assert!(manager.interface_hashes().contains(unrelated.address()));
    }
    assert_eq!(constructed.load(Ordering::SeqCst), 2);
    drop(handler);
    // Manager registrations may already be absent; handler cleanup must still run.
    stopping.await;
    for worker in workers {
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .expect("captured worker settled")
            .expect("captured worker joined");
    }
    assert_eq!(ended.load(Ordering::SeqCst), 2);
    let handler = transport.handler.lock().await;
    assert!(!handler.multicast_peer_routings.contains_key(&root));
    assert!(!handler.unicast_udp_ifaces.contains_key(&peer));
    assert_eq!(
        manager.lock().await.interface_hashes(),
        std::collections::HashSet::from([*unrelated.address()])
    );
}
