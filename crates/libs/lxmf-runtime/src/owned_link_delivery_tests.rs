use super::*;
use rand_core::OsRng;
use rns_transport::{
    destination::{link::LinkStatus, DestinationName, SingleOutputDestination},
    identity::PrivateIdentity,
    iface::IfaceRole,
    packet::PacketContext,
    resource::ResourceEventKind,
    transport::TransportConfig,
};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::runtime::Handle;

#[tokio::test]
async fn activation_budget_reaches_native_link_establishment_timer() {
    let owner = PrivateIdentity::new_from_rand(OsRng);
    let peer = PrivateIdentity::new_from_rand(OsRng);
    let transport = Arc::new(Transport::new(TransportConfig::new("slow-link", &owner, true)));
    let destination =
        SingleOutputDestination::new(*peer.as_identity(), DestinationName::new("lxmf", "delivery"))
            .desc;
    let task = {
        let transport = transport.clone();
        tokio::spawn(async move {
            crate::link_delivery::activate_link(&transport, destination, Duration::from_secs(75), 1)
                .await
        })
    };
    let link = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Some(link) = transport.delivery_link(&destination.address_hash).await {
                break link;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pending link published");
    let now = std::time::Instant::now();
    let guard = link.lock().await;
    assert!(
        !guard.establishment_timed_out(now + Duration::from_secs(20)),
        "a delayed LoRa proof must outlive the native default timer"
    );
    assert!(
        guard.establishment_timed_out(now + Duration::from_secs(76)),
        "the configured activation budget must still expire"
    );
    drop(guard);
    task.abort();
    let _ = task.await;
}

struct AdmissionProbe {
    events: tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    cleaned_before_release: Arc<AtomicBool>,
}
impl Drop for AdmissionProbe {
    fn drop(&mut self) {
        let cancelled = std::iter::from_fn(|| self.events.try_recv().ok())
            .any(|event| matches!(event.kind, ResourceEventKind::OutboundCancelled));
        self.cleaned_before_release.store(cancelled, Ordering::Release);
    }
}

#[tokio::test]
async fn retained_owner_cancels_registered_resource_before_releasing_admission() {
    for (timeout, disconnect) in [(false, false), (true, false), (false, true)] {
        let owner = PrivateIdentity::new_from_rand(OsRng);
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let transport =
            Arc::new(Transport::new(TransportConfig::new("owned-cancel", &owner, true)));
        let mut channel =
            transport.iface_manager().lock().await.new_channel_with_role(8, IfaceRole::Unicast);
        let iface = *channel.address();
        let destination = SingleOutputDestination::new(
            *signer.as_identity(),
            DestinationName::new("lxmf", "delivery"),
        )
        .desc;
        let (events, _) = tokio::sync::broadcast::channel(8);
        let link = transport
            .link_observed(destination, |outbound| {
                let request = outbound.request();
                let mut peer = Link::new_from_request(
                    &request,
                    signer.sign_key().clone(),
                    destination,
                    events,
                )
                .expect("valid test fixture or operation");
                outbound.handle_packet(&peer.prove(), iface);
                outbound.set_ingress_iface(iface);
                peer.handle_packet(&outbound.create_rtt(), iface);
            })
            .await;
        assert_eq!(link.lock().await.status(), LinkStatus::Active);
        while channel.tx_channel.try_recv().is_ok() {}
        let source = SingleOutputDestination::new(
            *owner.as_identity(),
            DestinationName::new("lxmf", "delivery"),
        )
        .desc
        .address_hash;
        let config = Arc::new(InProcessBackendConfig::new(
            "test",
            Handle::current(),
            transport.clone(),
            owner,
            source,
        ));
        let cleaned = Arc::new(AtomicBool::new(false));
        let admission = AdmissionProbe {
            events: transport.resource_events(),
            cleaned_before_release: cleaned.clone(),
        };
        let task = tokio::spawn(send_owned(OwnedSend {
            starting: None,
            link_connect_timeout: Duration::from_secs(1),
            link_connect_attempts: 1,
            config,
            destination,
            payload: vec![1; 2048],
            message_id: MessageId("aa".repeat(32)),
            method: DeliveryMethod::Direct,
            representation: MessageMethod::Resource,
            timeout: Duration::from_millis(if timeout { 100 } else { 5000 }),
            propagated_recipient: None,
            admission: Box::new(admission),
            completed: None,
        }));
        let advertisement = tokio::time::timeout(Duration::from_secs(2), channel.tx_channel.recv())
            .await
            .expect("valid test fixture or operation")
            .expect("valid test fixture or operation");
        assert_eq!(advertisement.packet.context, PacketContext::ResourceAdvrtisement);
        assert!(!cleaned.load(Ordering::Acquire));
        if disconnect {
            drop(channel);
        }
        if timeout {
            assert!(task.await.expect("valid test fixture or operation").is_err());
        } else {
            // Hold the native Link so cancellation cannot finish yet.
            let guard = link.lock().await;
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
            tokio::task::yield_now().await;
            assert!(!cleaned.load(Ordering::Acquire));
            drop(guard);
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while !cleaned.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("native cancellation precedes admission release");
    }
}

#[tokio::test]
async fn dispatch_authorization_waits_until_a_link_exists() {
    let owner = PrivateIdentity::new_from_rand(OsRng);
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let transport =
        Arc::new(Transport::new(TransportConfig::new("pre-dispatch-policy", &owner, false)));
    let source = SingleOutputDestination::new(
        *owner.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )
    .desc
    .address_hash;
    let destination = SingleOutputDestination::new(
        *signer.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )
    .desc;
    let config =
        Arc::new(InProcessBackendConfig::new("test", Handle::current(), transport, owner, source));
    let started = Arc::new(AtomicBool::new(false));
    let hook_started = started.clone();
    let result = send_owned(OwnedSend {
        link_connect_timeout: Duration::from_secs(1),
        link_connect_attempts: 1,
        starting: Some(Box::new(move || {
            hook_started.store(true, Ordering::Release);
            Ok(())
        })),
        config,
        destination,
        payload: vec![1; 2048],
        message_id: MessageId("aa".repeat(32)),
        method: DeliveryMethod::Direct,
        representation: MessageMethod::Resource,
        timeout: Duration::from_millis(5),
        propagated_recipient: None,
        admission: Box::new(()),
        completed: None,
    })
    .await;
    assert!(result.is_err());
    assert!(!started.load(Ordering::Acquire), "failed link setup cannot claim upload admission");
}
