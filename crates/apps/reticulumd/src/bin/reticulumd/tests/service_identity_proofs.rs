use super::*;
use rns_rpc::ServiceIdentityBridge;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn imported_created_and_restored_services_defer_delivery_proofs() {
    let (_daemon, bridge) = crate::tests::test_transport_bridge_fixture().await;
    let identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let imported = bridge
        .import_service_identity(&identity.to_private_key_bytes(), ServiceIdentitySpec::default())
        .expect("import");
    let created = bridge.create_service_identity(ServiceIdentitySpec::default()).expect("create");
    for record in [&imported, &created] {
        let registered = bridge
            .service_identity_for_destination(&record.delivery_destination)
            .expect("registered destination");
        assert!(registered.destination.lock().await.deferred_proofs());
    }
    let directory = bridge.service_identity_storage_dir();
    let (_, restarted, _, _) =
        crate::tests::test_transport_bridge_fixture_with_peer_at(directory).await;
    assert_eq!(restarted.load_persisted_service_identities().await.expect("restore"), 2);
    for record in [&imported, &created] {
        let restored = restarted
            .service_identity_for_destination(&record.delivery_destination)
            .expect("restored destination");
        assert!(restored.destination.lock().await.deferred_proofs());
    }
}
