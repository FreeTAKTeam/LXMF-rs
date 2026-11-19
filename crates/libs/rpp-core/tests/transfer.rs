//! Engine conformance (#767): standalone sender/receiver, restart with a
//! cached manifest, only-missing pulls, verified receipts, RC1 policy and
//! Resource negative cases, replay and concurrent-peer isolation.
mod common;

use common::*;
use rpp_core::control::{Body, Control, OfferDecision, ReceiptStatus, RppResult, Status};
use rpp_core::identity::sha256;
use rpp_core::inbox::answer_offer;
use rpp_core::publication::Publication;
use rpp_core::publisher::{LinkId, PublisherLimits};
use rpp_core::replay::ReplayCache;
use rpp_core::store::{ChunkStore, DirSource, FsStore, MemorySource, PackageSource};
use rpp_core::ReceiveDestination;
use rpp_core::{
    FileDraft, ManifestDraft, PublicIdentity, Publisher, Pull, PullConfig, RppError,
    SigningIdentity, VerifiedManifest,
};

const NOW: u64 = 1_700_000_000;

struct Setup {
    manifest: VerifiedManifest,
    chunks: Vec<Vec<u8>>,
    publication: Publication,
    owner: SigningIdentity,
    recipient: SigningIdentity,
}

fn resume_setup() -> Setup {
    let fixtures = fixtures();
    let owner = package_owner(&fixtures, "resume101");
    let publication = Publication::verify(
        &hex(&fixtures["resume_publication"]["Package"]["cbor_hex"]),
        &owner.public(),
    )
    .expect("fixture publication");
    let chunks = package_files(&fixtures, "resume101").remove(0).1;
    Setup {
        manifest: rebuild(&fixtures, "resume101"),
        chunks,
        publication,
        owner,
        recipient: identity(&fixtures, "second_fixture_identity_private_hex"),
    }
}

fn publisher<S: PackageSource>(manifest: &VerifiedManifest, source: S) -> Publisher<S> {
    let mut publisher = Publisher::new(Box::new(|_| true), PublisherLimits::default(), NOW);
    publisher.publish(manifest.clone(), source);
    publisher
}

fn pull(
    setup: &Setup,
    me: &PublicIdentity,
    dir: &std::path::Path,
    max_bundle: u64,
) -> Pull<FsStore> {
    let binding = rpp_core::OwnerBinding {
        owner: setup.owner.public(),
        package_destination: setup.manifest.manifest.package_destination,
        manifest_destination: setup.manifest.manifest.manifest_destination,
    };
    let config = PullConfig { max_bundle, window: 2, ..PullConfig::default() };
    Pull::new(
        FsStore::open(dir).expect("test step"),
        me,
        binding,
        setup.publication.clone(),
        config,
    )
    .expect("test step")
}

#[derive(Default, Debug)]
struct Stats {
    manifest_bytes: usize,
    bundle_bytes: usize,
    bundles: usize,
}

/// Drives requests, responses and Resources in-process over one Link.
/// Stops early (simulated crash) after `crash_after` bundles.
fn drive<S: PackageSource>(
    pull: &mut Pull<FsStore>,
    publisher: &mut Publisher<S>,
    me: &PublicIdentity,
    link: LinkId,
    crash_after: Option<usize>,
) -> Stats {
    let destination = resume_setup().manifest.manifest.package_destination;
    let mut stats = Stats::default();
    for _ in 0..1000 {
        let mut queue = pull.next_requests(NOW).expect("next requests");
        if queue.is_empty() {
            return stats;
        }
        while let Some(request) = queue.pop() {
            let served =
                publisher.handle(link, destination, Some(me), &request, NOW).expect("served");
            // Deliver the Resource before the response half the time to
            // exercise quarantine-until-APPLIED.
            let early = stats.bundles % 2 == 1;
            if early {
                if let Some(out) = &served.outbound {
                    pull.on_resource(&out.metadata, &out.payload).expect("early resource");
                }
            }
            queue.extend(pull.on_response(&served.response, NOW).expect("response"));
            if let Some(out) = served.outbound {
                publisher.resource_concluded(link);
                if !early {
                    pull.on_resource(&out.metadata, &out.payload).expect("resource");
                }
                if pull.manifest().is_some_and(|manifest| manifest.bytes == out.payload) {
                    stats.manifest_bytes += out.payload.len();
                } else {
                    stats.bundles += 1;
                    stats.bundle_bytes += out.payload.len();
                }
                if crash_after == Some(stats.bundles) {
                    return stats;
                }
            }
        }
    }
    panic!("transfer did not settle");
}

#[test]
fn standalone_transfer_survives_restart_and_pulls_only_missing_chunks() {
    let setup = resume_setup();
    let me = setup.recipient.public();
    let dir = tempfile::tempdir().expect("test step");
    let mut publisher = publisher(&setup.manifest, MemorySource(setup.chunks.clone()));

    let mut first = pull(&setup, &me, dir.path(), 10);
    let stats = drive(&mut first, &mut publisher, &me, [1; 16], Some(4));
    assert_eq!(stats.manifest_bytes, setup.manifest.bytes.len());
    assert_eq!(stats.bundle_bytes, 40);
    drop(first); // process crash: only the store survives
    publisher.link_closed([1; 16]);

    let mut resumed = pull(&setup, &me, dir.path(), 10);
    assert_eq!(resumed.progress().verified_chunks, 40, "verified chunks survive restart");
    assert!(resumed.progress().has_manifest, "manifest cache hit");
    let stats = drive(&mut resumed, &mut publisher, &me, [2; 16], None);
    assert_eq!(stats.manifest_bytes, 0, "manifest is not fetched again");
    assert_eq!(stats.bundle_bytes, 61, "only missing chunks travel");
    let progress = resumed.progress();
    assert!(progress.complete_verified && progress.receipt_acknowledged);
    let receipt = publisher.receipts().last().expect("receipt");
    assert_eq!((receipt.status, receipt.verified_count), (ReceiptStatus::CompleteVerified, 101));
    let store = FsStore::open(dir.path()).expect("test step");
    let file = std::fs::read(store.package_dir(&setup.manifest.hash).join("series.bin"))
        .expect("test step");
    assert_eq!(file, setup.chunks.concat());
}

#[test]
fn fixture_resume_transfers_two_bytes_for_indexes_17_and_63() {
    let setup = resume_setup();
    let fixtures = fixtures();
    let me = setup.recipient.public();
    let dir = tempfile::tempdir().expect("test step");
    let mut store = FsStore::open(dir.path()).expect("test step");
    store.save_manifest(&setup.manifest).expect("test step");
    for index in (0..101u32).filter(|i| *i != 17 && *i != 63) {
        store
            .write_chunk(&setup.manifest, index, &setup.chunks[index as usize])
            .expect("test step");
    }
    // false_completion: with 100 of 101 chunks verified the recipient
    // never creates a TransferReceipt; it asks for the gap instead.
    store.write_chunk(&setup.manifest, 17, &setup.chunks[17]).expect("test step");
    let mut short = pull(&setup, &me, dir.path(), 32);
    assert_eq!(short.progress().verified_chunks, 100);
    let only = Control::decode(&short.next_requests(NOW).expect("test step").remove(0))
        .expect("test step");
    assert!(matches!(only.body, Body::MissingChunks { .. }), "no receipt from a partial inventory");
    std::fs::remove_dir_all(dir.path().join("staging")).expect("test step");
    for index in (0..101u32).filter(|i| *i != 17 && *i != 63) {
        store
            .write_chunk(&setup.manifest, index, &setup.chunks[index as usize])
            .expect("test step");
    }

    let mut pull = pull(&setup, &me, dir.path(), 32);
    let request = pull.next_requests(NOW).expect("test step").remove(0);
    let Body::MissingChunks { selection, .. } = Control::decode(&request).expect("test step").body
    else {
        panic!("expected MissingChunks");
    };
    assert_eq!(selection.indexes().collect::<Vec<_>>(), vec![17, 63]);

    let mut publisher = publisher(&setup.manifest, MemorySource(setup.chunks.clone()));
    let link = [7; 16];
    let destination = setup.manifest.manifest.package_destination;
    let served = publisher.handle(link, destination, Some(&me), &request, NOW).expect("test step");
    let accept = pull.on_response(&served.response, NOW).expect("test step").remove(0);
    let served = publisher.handle(link, destination, Some(&me), &accept, NOW).expect("test step");
    let bundle = served.outbound.clone().expect("bundle Resource");
    assert_eq!(bundle.payload, hex(&fixtures["resume"]["payload_hex"]));
    assert_eq!(rpp_core::pack_resource_metadata(&[0x35; 16]).len(), 18);
    pull.on_response(&served.response, NOW).expect("test step");
    pull.on_resource(&bundle.metadata, &bundle.payload).expect("test step");
    // A repeated identical Resource is acknowledged once, idempotently.
    pull.on_resource(&bundle.metadata, &bundle.payload).expect("test step");
    let receipt = pull.next_requests(NOW).expect("test step").remove(0);
    let served = publisher.handle(link, destination, Some(&me), &receipt, NOW).expect("test step");
    pull.on_response(&served.response, NOW).expect("test step");
    assert!(pull.progress().receipt_acknowledged);
}

#[test]
fn rns_metadata_matches_fixture_packing() {
    let fixtures = fixtures();
    let id: [u8; 16] = fixed(hex(&fixtures["resume"]["resource_metadata_value_hex"]));
    let packed = rpp_core::pack_resource_metadata(&id);
    assert_eq!(packed, hex(&fixtures["resume"]["rns_packed_metadata_hex"]));
    assert_eq!(rpp_core::unpack_resource_metadata(&packed), Ok(id));
}

#[test]
fn publisher_reproduces_fixture_results_and_policy_refusals() {
    let setup = resume_setup();
    let fixtures = fixtures();
    let me = setup.recipient.public();
    assert_eq!(
        me.receive_destination().to_vec(),
        diag_bytes(&fixtures["commands"]["Offer"]["value"]["5"])
    );
    let mut publisher = publisher(&setup.manifest, MemorySource(setup.chunks.clone()));
    let link = [9; 16];
    let destination = setup.manifest.manifest.package_destination;
    for name in ["ManifestRequest", "MissingChunks", "ResourceAccept", "TransferReceipt"] {
        let request = hex(&fixtures["commands"][name]["cbor_hex"]);
        let served = publisher.handle(link, destination, Some(&me), &request, NOW).expect(name);
        assert_eq!(served.response, hex(&fixtures["results"][name]["cbor_hex"]), "{name}");
        if served.outbound.is_some() {
            publisher.resource_concluded(link);
        }
        // Identical retry: same bytes back, no second send.
        let retry =
            publisher.handle(link, destination, Some(&me), &request, NOW).expect("test step");
        assert_eq!((retry.response, retry.outbound), (served.response, None), "{name} retry");
    }

    let status = |served: Option<rpp_core::Served>| {
        RppResult::decode(&served.expect("test step").response).expect("test step").status
    };
    let negatives = fixtures["negative"].as_array().expect("test step");
    let case =
        |name: &str| negatives.iter().find(|c| c["case"] == name).expect("test step").clone();

    let unauthorized = case("unauthorized_pull");
    let publisher_identity = PublicIdentity(fixed(hex(&unauthorized["identified_public_hex"])));
    let served = publisher.handle(
        link,
        destination,
        Some(&publisher_identity),
        &hex(&unauthorized["input_hex"]),
        NOW,
    );
    assert_eq!(status(served), Status::NotAuthorized);

    let expired = case("expired_pull");
    let when = expired["receive_time"].as_u64().expect("test step");
    let served =
        publisher.handle([10; 16], destination, Some(&me), &hex(&expired["input_hex"]), when);
    assert_eq!(status(served), Status::Rejected);

    // A crafted complete_verified receipt for 100 of 101 chunks is refused.
    let mut short =
        Control::decode(&hex(&case("false_completion")["input_hex"])).expect("test step");
    short.header.request_id = [0x38; 16];
    if let Body::TransferReceipt { verified_count, .. } = &mut short.body {
        *verified_count = 100;
    }
    let served = publisher.handle([10; 16], destination, Some(&me), &short.encode(), NOW);
    assert_eq!(status(served), Status::Rejected, "receipt claiming 100/101 is not completion");

    // Conflicting reuse of a request ID is REJECTED; unidentified traffic is dropped.
    let mut conflicting = Control::decode(&hex(&fixtures["commands"]["MissingChunks"]["cbor_hex"]))
        .expect("test step");
    if let Body::MissingChunks { max_bundle, .. } = &mut conflicting.body {
        *max_bundle = 31;
    }
    let served = publisher.handle(link, destination, Some(&me), &conflicting.encode(), NOW);
    assert_eq!(status(served), Status::Rejected);
    assert!(publisher.handle(link, destination, None, &conflicting.encode(), NOW).is_none());
    assert!(publisher.handle(link, destination, Some(&me), b"\xff", NOW).is_none());
}

#[test]
fn inbox_answers_fixture_offer_and_refuses_non_owners() {
    let setup = resume_setup();
    let fixtures = fixtures();
    let me = setup.recipient.public();
    let owner = setup.owner.public();
    let offer = hex(&fixtures["commands"]["Offer"]["cbor_hex"]);
    let package = setup.manifest.manifest.package_destination;
    let resolve = |destination: &rpp_core::Destination| (*destination == package).then_some(owner);
    let mut replay = ReplayCache::new(NOW, 64);
    let response = answer_offer(&mut replay, &me, Some(&owner), &offer, NOW, resolve, |_| {
        OfferDecision::Accepted
    });
    assert_eq!(response.expect("test step"), hex(&fixtures["results"]["Offer"]["cbor_hex"]));
    let response = answer_offer(&mut replay, &me, Some(&me), &offer, NOW, resolve, |_| {
        OfferDecision::Accepted
    });
    assert_eq!(
        RppResult::decode(&response.expect("test step")).expect("test step").status,
        Status::NotAuthorized
    );
}

/// A malicious publisher: wrong-owner manifest, oversized plan, corrupt
/// chunk, conflicting duplicate and unassociated Resources.
#[test]
fn receiver_rejects_malicious_publisher_input() {
    let setup = resume_setup();
    let fixtures = fixtures();
    let me = setup.recipient.public();
    let dir = tempfile::tempdir().expect("test step");

    // Manifest signed by someone else never verifies or gets cached.
    let mut fresh = pull(&setup, &me, dir.path(), 32);
    let request = fresh.next_requests(NOW).expect("test step").remove(0);
    let id = Control::decode(&request).expect("test step").header.request_id;
    let applied = RppResult { request_id: id, status: Status::Applied, value: None }.encode();
    fresh.on_response(&applied, NOW).expect("test step");
    let forged = rebuild(&fixtures, "second_owner");
    assert!(fresh.on_resource(&id, &forged.bytes).is_err());
    assert!(!fresh.progress().has_manifest);

    let mut store = FsStore::open(dir.path()).expect("test step");
    store.save_manifest(&setup.manifest).expect("test step");
    for index in (0..101u32).filter(|i| *i != 17 && *i != 63) {
        store
            .write_chunk(&setup.manifest, index, &setup.chunks[index as usize])
            .expect("test step");
    }
    let mut pull = pull(&setup, &me, dir.path(), 1);
    let request =
        Control::decode(&pull.next_requests(NOW).expect("test step").remove(0)).expect("test step");
    let plan_reply = |payload: &[u8], payload_len: u64| {
        let plan = Control {
            header: request.header,
            body: Body::ResourcePlan {
                indexes: vec![17, 63],
                payload_len,
                bundle_digest: sha256(payload),
                plan_id: request.header.request_id,
            },
        };
        RppResult {
            request_id: request.header.request_id,
            status: Status::Applied,
            value: Some(plan.encode()),
        }
        .encode()
    };
    // exceeds_budget: plan larger than the accepted maximum (1 byte here).
    assert_eq!(pull.on_response(&plan_reply(&[0x11, 0x3f], 2), NOW), Err(RppError::LimitExceeded));

    let mut pull = self::pull(&setup, &me, dir.path(), 32);
    let request =
        Control::decode(&pull.next_requests(NOW).expect("test step").remove(0)).expect("test step");
    let corrupt = hex(&negative(&fixtures, "corrupt_chunk")["input_hex"]);
    let reply = {
        let plan = Control {
            header: request.header,
            body: Body::ResourcePlan {
                indexes: vec![17, 63],
                payload_len: 2,
                bundle_digest: sha256(&corrupt),
                plan_id: request.header.request_id,
            },
        };
        RppResult {
            request_id: request.header.request_id,
            status: Status::Applied,
            value: Some(plan.encode()),
        }
        .encode()
    };
    let accept = pull.on_response(&reply, NOW).expect("test step").remove(0);
    let accept_id = Control::decode(&accept).expect("test step").header.request_id;
    pull.on_response(
        &RppResult { request_id: accept_id, status: Status::Applied, value: None }.encode(),
        NOW,
    )
    .expect("test step");
    let plan_id = request.header.request_id;
    assert_eq!(pull.on_resource(&plan_id, &corrupt), Err(RppError::ChunkMismatch));
    assert_eq!(pull.progress().verified_chunks, 99, "no index marked present from a bad bundle");

    let unassociated: [u8; 16] =
        fixed(hex(&negative(&fixtures, "unassociated_resource")["metadata_hex"]));
    assert_eq!(pull.on_resource(&unassociated, &[0x11, 0x3f]), Err(RppError::NoAcceptedPlan));
}

#[test]
fn conflicting_duplicate_resource_is_discarded() {
    let setup = resume_setup();
    let fixtures = fixtures();
    let me = setup.recipient.public();
    let dir = tempfile::tempdir().expect("test step");
    let mut store = FsStore::open(dir.path()).expect("test step");
    store.save_manifest(&setup.manifest).expect("test step");
    for index in (0..101u32).filter(|i| *i != 17 && *i != 63) {
        store
            .write_chunk(&setup.manifest, index, &setup.chunks[index as usize])
            .expect("test step");
    }
    let mut pull = pull(&setup, &me, dir.path(), 32);
    let mut publisher = publisher(&setup.manifest, MemorySource(setup.chunks.clone()));
    let destination = setup.manifest.manifest.package_destination;
    let request = pull.next_requests(NOW).expect("test step").remove(0);
    let served =
        publisher.handle([3; 16], destination, Some(&me), &request, NOW).expect("test step");
    let accept = pull.on_response(&served.response, NOW).expect("test step").remove(0);
    let served =
        publisher.handle([3; 16], destination, Some(&me), &accept, NOW).expect("test step");
    pull.on_response(&served.response, NOW).expect("test step");
    let bundle = served.outbound.expect("test step");
    pull.on_resource(&bundle.metadata, &bundle.payload).expect("test step");
    let conflicting = hex(&negative(&fixtures, "conflicting_resource")["input_hex"]);
    assert_eq!(
        pull.on_resource(&bundle.metadata, &conflicting),
        Err(RppError::ConflictingDuplicate)
    );
    assert_eq!(pull.progress().verified_chunks, 101, "verified bytes are preserved");
}

#[test]
fn concurrent_receivers_on_separate_links_are_isolated() {
    let setup = resume_setup();
    let mut publisher = publisher(&setup.manifest, MemorySource(setup.chunks.clone()));
    let peers: Vec<SigningIdentity> =
        (1..=3u8).map(|seed| SigningIdentity::from_private_bytes(&[seed; 64])).collect();
    let dirs: Vec<_> = peers.iter().map(|_| tempfile::tempdir().expect("test step")).collect();
    let mut pulls: Vec<_> = peers
        .iter()
        .zip(&dirs)
        .map(|(peer, dir)| pull(&setup, &peer.public(), dir.path(), 16))
        .collect();
    // Round-robin one request per peer per step.
    let destination = setup.manifest.manifest.package_destination;
    let mut queues: Vec<Vec<Vec<u8>>> = vec![Vec::new(); peers.len()];
    for _ in 0..500 {
        for (slot, pull) in pulls.iter_mut().enumerate() {
            let me = peers[slot].public();
            let link = [slot as u8 + 40; 16];
            if queues[slot].is_empty() {
                queues[slot] = pull.next_requests(NOW).expect("test step");
            }
            if let Some(request) = queues[slot].pop() {
                let served = publisher
                    .handle(link, destination, Some(&me), &request, NOW)
                    .expect("test step");
                queues[slot].extend(pull.on_response(&served.response, NOW).expect("test step"));
                if let Some(out) = served.outbound {
                    publisher.resource_concluded(link);
                    pull.on_resource(&out.metadata, &out.payload).expect("test step");
                }
            }
        }
    }
    assert!(pulls.iter().all(|pull| pull.progress().receipt_acknowledged));
    assert_eq!(publisher.receipts().len(), peers.len());
}

#[test]
fn large_disk_file_round_trip_with_dir_source() {
    let source_dir = tempfile::tempdir().expect("test step");
    let data: Vec<u8> = (0..3 * 1024 * 1024 + 17).map(|i| (i * 31 % 251) as u8).collect();
    std::fs::create_dir_all(source_dir.path().join("maps")).expect("test step");
    std::fs::write(source_dir.path().join("maps/area.bin"), &data).expect("test step");
    let owner = SigningIdentity::from_private_bytes(&[9; 64]);
    let file = std::fs::File::open(source_dir.path().join("maps/area.bin")).expect("test step");
    let draft =
        FileDraft::from_reader("maps/area.bin", [4; 16], file, 256 * 1024).expect("test step");
    let manifest = ManifestDraft {
        package_destination: [1; 16],
        manifest_destination: [2; 16],
        files: vec![draft, FileDraft::from_chunks("readme.txt", [5; 16], &[b"hi"])],
        ..ManifestDraft::default()
    }
    .sign(&owner)
    .expect("test step");
    std::fs::write(source_dir.path().join("readme.txt"), b"hi").expect("test step");
    assert_eq!(manifest.chunk_count(), 14);
    let publication = Publication {
        kind: rpp_core::publication::PublicationKind::Package,
        object: [1; 16],
        revision_id: [0; 32],
        package_destination: [1; 16],
        manifest_destination: [2; 16],
        package_id: manifest.manifest.package_id,
        manifest_hash: manifest.hash,
        file: None,
    };
    let setup = Setup {
        manifest: manifest.clone(),
        chunks: Vec::new(),
        publication,
        owner,
        recipient: SigningIdentity::from_private_bytes(&[8; 64]),
    };
    let me = setup.recipient.public();
    let dir = tempfile::tempdir().expect("test step");
    let mut publisher = publisher(&manifest, DirSource { root: source_dir.path().to_path_buf() });
    let mut pull = pull(&setup, &me, dir.path(), 1024 * 1024);
    let mut queue = Vec::new();
    for _ in 0..200 {
        if queue.is_empty() {
            queue = pull.next_requests(NOW).expect("test step");
            if queue.is_empty() {
                break;
            }
        }
        let request = queue.pop().expect("test step");
        let served =
            publisher.handle([6; 16], [1; 16], Some(&me), &request, NOW).expect("test step");
        queue.extend(pull.on_response(&served.response, NOW).expect("test step"));
        if let Some(out) = served.outbound {
            assert!(
                out.payload.len() <= 1024 * 1024 + 4096,
                "bundles stay within the negotiated budget"
            );
            publisher.resource_concluded([6; 16]);
            pull.on_resource(&out.metadata, &out.payload).expect("test step");
        }
    }
    assert!(pull.progress().receipt_acknowledged);
    let store = FsStore::open(dir.path()).expect("test step");
    assert_eq!(
        std::fs::read(store.package_dir(&manifest.hash).join("maps/area.bin")).expect("test step"),
        data
    );
}

fn negative(fixtures: &serde_json::Value, name: &str) -> serde_json::Value {
    fixtures["negative"]
        .as_array()
        .expect("test step")
        .iter()
        .find(|c| c["case"] == name)
        .expect("test step")
        .clone()
}

/// Found over real RNS: an APPLIED ManifestRequest whose Resource has not
/// landed yet must not trigger another ManifestRequest.
#[test]
fn applied_manifest_request_is_not_repeated_while_resource_is_in_flight() {
    let setup = resume_setup();
    let me = setup.recipient.public();
    let dir = tempfile::tempdir().expect("test step");
    let mut pull = pull(&setup, &me, dir.path(), 32);
    let request =
        Control::decode(&pull.next_requests(NOW).expect("test step").remove(0)).expect("test step");
    let id = request.header.request_id;
    pull.on_response(
        &RppResult { request_id: id, status: Status::Applied, value: None }.encode(),
        NOW,
    )
    .expect("test step");
    assert!(pull.next_requests(NOW).expect("test step").is_empty());
    pull.on_resource(&id, &setup.manifest.bytes).expect("test step");
    assert!(pull.progress().has_manifest);
}

/// An abandoned plan stops holding a window slot once it expires.
#[test]
fn expired_unaccepted_plan_frees_the_window() {
    let setup = resume_setup();
    let fixtures = fixtures();
    let me = setup.recipient.public();
    let mut publisher = publisher(&setup.manifest, MemorySource(setup.chunks.clone()));
    let destination = setup.manifest.manifest.package_destination;
    let link = [12; 16];
    let first = hex(&fixtures["commands"]["MissingChunks"]["cbor_hex"]); // window 1
    let status = |served: Option<rpp_core::Served>| {
        RppResult::decode(&served.expect("served").response).expect("result").status
    };
    assert_eq!(
        status(publisher.handle(link, destination, Some(&me), &first, NOW)),
        Status::Applied
    );

    let mut later = Control::decode(&first).expect("control");
    later.header.request_id = [0x40; 16];
    later.header.issued = NOW + 200;
    later.header.expires = NOW + 300;
    let served = publisher.handle(link, destination, Some(&me), &later.encode(), NOW + 200);
    assert_eq!(status(served), Status::Applied, "expired plan no longer blocks the Link");
}
