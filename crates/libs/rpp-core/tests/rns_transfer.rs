//! #767 over real RNS: two transports on a loopback TCP interface, an
//! identified Link to the RDO Package Destination, `/rpp/3` Link requests
//! and native uncompressed Resources. The first pull is cut off mid-transfer
//! (the publisher withdraws authorization); a restarted receiver opens a new
//! Link, reuses its cached manifest and pulls only the missing chunks.

use std::net::{Ipv4Addr, TcpListener};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rand_core::OsRng;
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::transport::{Transport, TransportConfig};
use rpp_core::control::ReceiptStatus;
use rpp_core::identity::destination_hash;
use rpp_core::publication::{Publication, PublicationKind};
use rpp_core::publisher::PublisherLimits;
use rpp_core::rns::{public_identity, rdo_object_name, run_pull, system_clock, PublisherService};
use rpp_core::store::{FsStore, MemorySource};
use rpp_core::{
    FileDraft, ManifestDraft, OwnerBinding, Publisher, Pull, PullConfig, SigningIdentity,
};

const TIMEOUT: Duration = Duration::from_secs(20);

fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve port");
    listener.local_addr().expect("local addr").port()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rpp_pull_over_rns_link_resumes_with_cached_manifest() {
    // Publisher: owner Identity, Package Destination and a 50-chunk file.
    let owner = PrivateIdentity::new_from_rand(OsRng);
    let signer = SigningIdentity::from_private_bytes(&owner.to_private_key_bytes());
    assert_eq!(signer.public(), public_identity(owner.as_identity()));
    let token = [0x11; 16];
    let package_destination =
        destination_hash(&signer.public().0, &format!("rdo.object.{}", hex::encode(token)));
    let data: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
    let chunks: Vec<Vec<u8>> = data.chunks(4096).map(<[u8]>::to_vec).collect();
    let refs: Vec<&[u8]> = chunks.iter().map(Vec::as_slice).collect();
    let manifest = ManifestDraft {
        package_destination,
        manifest_destination: [0x22; 16],
        files: vec![FileDraft::from_chunks("data/series.bin", [0x33; 16], &refs)],
        ..ManifestDraft::default()
    }
    .sign(&signer)
    .expect("manifest");

    let port = free_port();
    let server = Arc::new(Transport::new(TransportConfig::new("rpp-pub", &owner, false)));
    let manager = server.iface_manager();
    manager
        .lock()
        .await
        .spawn(TcpServer::new(format!("127.0.0.1:{port}"), manager.clone()), TcpServer::spawn);
    let destination = server.add_destination(owner.clone(), rdo_object_name(&token)).await;
    assert_eq!(destination.lock().await.desc.address_hash.as_slice(), package_destination);

    // Policy: allow three MissingChunks, then deny until reopened.
    let open = Arc::new(AtomicBool::new(true));
    let missing_served = Arc::new(AtomicUsize::new(0));
    let manifest_requests = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let policy = {
        let (open, missing_served, manifest_requests) =
            (open.clone(), missing_served.clone(), manifest_requests.clone());
        Box::new(move |request: &rpp_core::publisher::AuthRequest<'_>| {
            match request.control.body.command() {
                3 => {
                    // Replayed retries re-run policy; count distinct requests.
                    let id = request.control.header.request_id;
                    manifest_requests.lock().expect("ids").insert(id);
                }
                7 if open.load(Ordering::SeqCst)
                    && missing_served.fetch_add(1, Ordering::SeqCst) == 2 =>
                {
                    open.store(false, Ordering::SeqCst);
                }
                _ => {}
            }
            open.load(Ordering::SeqCst) || request.control.body.command() != 7
        })
    };
    let mut engine = Publisher::new(policy, PublisherLimits::default(), system_clock() - 5);
    engine.publish(manifest.clone(), MemorySource(chunks.clone()));
    let service = Arc::new(PublisherService::new(server.clone(), engine, system_clock));
    let runner = service.clone();
    tokio::spawn(async move { runner.run().await });
    let announcer = server.clone();
    tokio::spawn(async move {
        loop {
            announcer.send_announce(&destination, None).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    // Receiver: resolve the Package Destination from its announce.
    let me = PrivateIdentity::new_from_rand(OsRng);
    let client = Transport::new(TransportConfig::new("rpp-recv", &me, false));
    let mut announces = client.recv_announces().await;
    client
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{port}")), TcpClient::spawn);
    let package = tokio::time::timeout(TIMEOUT, async {
        loop {
            let event = announces.recv().await.expect("announce");
            let desc = event.destination.lock().await.desc;
            if desc.address_hash.as_slice() == package_destination {
                return desc;
            }
        }
    })
    .await
    .expect("package announce");

    let binding = OwnerBinding {
        owner: signer.public(),
        package_destination,
        manifest_destination: [0x22; 16],
    };
    let publication = Publication {
        kind: PublicationKind::Package,
        object: package_destination,
        revision_id: [0; 32],
        package_destination,
        manifest_destination: [0x22; 16],
        package_id: manifest.manifest.package_id,
        manifest_hash: manifest.hash,
        file: None,
    };
    let store_dir = tempfile::tempdir().expect("store");
    let config = PullConfig { max_bundle: 16 * 1024, window: 2, ..PullConfig::default() };
    let my_public = public_identity(me.as_identity());
    let new_pull = || {
        let store = FsStore::open(store_dir.path()).expect("store");
        Pull::new(store, &my_public, binding, publication.clone(), config).expect("pull")
    };

    let mut first = new_pull();
    let interrupted = run_pull(&client, &me, package, &mut first, TIMEOUT, system_clock).await;
    assert_eq!(interrupted.err(), Some(rpp_core::RppError::NotAuthorized));
    let partial = first.progress();
    assert!(partial.has_manifest && partial.verified_chunks > 0 && partial.verified_chunks < 49);
    drop(first); // receiver restart

    open.store(true, Ordering::SeqCst);
    let mut resumed = new_pull();
    assert_eq!(resumed.progress().verified_chunks, partial.verified_chunks);
    let done =
        run_pull(&client, &me, package, &mut resumed, TIMEOUT, system_clock).await.expect("resume");
    assert!(done.complete_verified && done.receipt_acknowledged);
    let distinct = manifest_requests.lock().expect("ids").len();
    assert_eq!(distinct, 1, "manifest fetched once, reused after restart");

    let engine = service.engine();
    let receipts = engine.lock().expect("engine").receipts().to_vec();
    assert_eq!(receipts.last().map(|r| r.status), Some(ReceiptStatus::CompleteVerified));
    let store = FsStore::open(store_dir.path()).expect("store");
    let promoted =
        std::fs::read(store.package_dir(&manifest.hash).join("data/series.bin")).expect("file");
    assert_eq!(promoted, data);
}
