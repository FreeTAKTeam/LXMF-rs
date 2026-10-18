use super::*;

#[tokio::test]
async fn committed_fallback_retires_direct_sending_disposition() {
    let mut task = crate::bridge::delivery_task_tests::delivery_task_for_propagation_cost_lookup(
        Arc::new(rns_rpc::RpcDaemon::test_instance()),
    );
    task.requested_method = RequestedDeliveryMethod::Direct;
    task.try_propagation_on_fail = true;
    task.propagation_node_hex = Some(hex::encode([7; 16]));
    let identity = rns_transport::identity::PrivateIdentity::new_from_name("fallback-transfer");
    let link = Arc::new(Mutex::new(Link::new(
        DestinationDesc {
            identity: *identity.as_identity(),
            address_hash: task.destination_hash,
            name: DestinationName::new("lxmf", "delivery"),
        },
        tokio::sync::broadcast::channel(8).0,
    )));
    let mut retry = state::Retry { attempts: 5, ..Default::default() };
    let mut context = Some(Context {
        link: link.clone(),
        peer: task.destination_hex.clone(),
        method: Method::Direct,
        identify: false,
        signal: None,
    });
    let mut transfer = Some(Transfer::NoReceipt { link });
    let mut sending = true;
    assert!(
        fallback(&mut task, Method::Direct, &mut retry, &mut context, &mut transfer, &mut sending)
            .await
    );
    assert!(!sending, "an active propagation Link must be allowed to dispatch");
    assert!(transfer.is_none(), "the prior method no longer owns a current transfer");
    assert!(context.is_none());
    assert_eq!(retry.attempts, 0);
    assert_eq!(task.requested_method, RequestedDeliveryMethod::Propagated);
    // Prior packet proof tokens deliberately live outside this method transition.
}
