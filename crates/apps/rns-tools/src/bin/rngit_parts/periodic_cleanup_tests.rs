use super::*;
use rns_transport::destination::DestinationName;
use rns_transport::identity::PrivateIdentity;
use rns_transport::transport::TransportConfig;
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn periodic_sweep_removes_media_for_silently_disappeared_link() {
    let identity = PrivateIdentity::new_from_name("rngit-periodic-cleanup-test");
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "rngit-cleanup-test",
        &identity,
        true,
    )));
    let destination = transport
        .add_destination(identity.clone(), DestinationName::new("nomadnetwork", "node"))
        .await;
    let git_destination = transport
        .add_destination(identity, DestinationName::new("git", "repositories"))
        .await;
    let mut node = ReticulumGitNode::default();
    let link_id = [0x61; 16];
    node.page_link_connected(link_id);
    let page_destination_hash = destination.lock().await.desc.address_hash;
    let git_destination_hash = git_destination.lock().await.desc.address_hash;
    let runtime = Runtime {
        transport,
        destination,
        git_destination,
        page_destination_hash,
        git_destination_hash,
        node: Arc::new(Mutex::new(node)),
        announce_app_data: Vec::new(),
        silent: true,
    };
    let observed_node = runtime.node.clone();

    // Consume the loop's immediate interval ticks, then model a peer that
    // disappears silently without a close event or failed response write.
    let server = tokio::spawn(serve_with_intervals(
        runtime,
        Duration::from_secs(3600),
        Duration::from_secs(60),
    ));
    tokio::task::yield_now().await;
    let directory = {
        let mut node = observed_node.lock().await;
        node.next_media_directory(link_id).expect("temporary media directory")
    };
    assert!(directory.is_dir());

    tokio::time::advance(Duration::from_secs(60)).await;
    tokio::task::yield_now().await;
    assert!(!directory.exists(), "periodic sweep should remove stale-link media");
    assert!(!observed_node.lock().await.active_page_links.contains_key(&link_id));
    server.abort();
}
