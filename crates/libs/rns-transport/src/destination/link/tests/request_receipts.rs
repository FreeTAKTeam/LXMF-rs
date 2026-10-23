use super::*;
use crate::destination::DestinationName;

fn response_envelope(id: &[u8], data: &[u8]) -> Vec<u8> {
    let value = rmpv::Value::Array(vec![
        rmpv::Value::Binary(id.to_vec()),
        rmpv::Value::Binary(data.to_vec()),
    ]);
    let mut packed = Vec::new();
    rmpv::encode::write_value(&mut packed, &value).expect("response envelope");
    packed
}

#[test]
fn oversized_response_cannot_reopen_the_same_request_with_a_later_valid_packet() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "response-limit"),
    };
    let (events, mut seen) = tokio::sync::broadcast::channel(8);
    let mut local = Link::new(destination, events.clone());
    let request = local.request();
    let mut peer = Link::new_from_request(&request, signer.sign_key().clone(), destination, events)
        .expect("native responder");
    let iface = AddressHash::new([37; ADDRESS_HASH_SIZE]);
    assert!(matches!(local.handle_packet(&peer.prove(), iface), LinkHandleResult::Activated));
    while seen.try_recv().is_ok() {}
    let request = Link::request_payload("/echo", rmpv::Value::Nil).expect("request payload");
    let packet = local
        .request_packet_with_max_response_size(&request.packed, Some(4))
        .expect("bounded native request");
    let id = &packet.hash().to_bytes()[..ADDRESS_HASH_SIZE];
    for response_data in [vec![1; 100], vec![2]] {
        let response = peer
            .response_packet(&response_envelope(id, &response_data))
            .expect("encrypted response");
        local.handle_packet(&response, iface);
    }
    while let Ok(event) = seen.try_recv() {
        assert!(
            !matches!(event.event, LinkEvent::Data(_)),
            "a rejected request cannot become an unbounded later response"
        );
    }
}

#[test]
#[ignore = "requires exact RNS 1.5.5 checkout and reference Python"]
fn frozen_1_5_5_request_pending_guards_ack_and_resource_delivery() {
    let repo = std::env::var("RETICULUM_PY_REPO").expect("explicit target reference checkout");
    let head = std::process::Command::new("git")
        .args(["-C", &repo, "rev-parse", "HEAD"])
        .output()
        .expect("reference revision");
    assert!(head.status.success());
    assert_eq!(
        String::from_utf8(head.stdout).expect("revision text").trim(),
        "7f2b3b9b524c9386316379af1313b43a5e4f7a5d"
    );
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into());
    let script = r#"
import json, sys, time
from types import SimpleNamespace
sys.path.insert(0, sys.argv[1])
import RNS
from RNS.Link import RequestReceipt

def receipt(state):
    link = SimpleNamespace(pending_requests=[])
    q = RequestReceipt.__new__(RequestReceipt)
    q.link, q.status, q.concluded_at = link, state, None
    q.failures = 0
    def failed(_): q.failures += 1
    q.callbacks = SimpleNamespace(failed=failed, progress=None, response=None)
    link.pending_requests.append(q)
    return q

rows = []
for state in [RequestReceipt.SENT, RequestReceipt.DELIVERED, RequestReceipt.RECEIVING]:
    for method in ['request_timed_out', 'response_rejected']:
        q = receipt(state)
        for _ in range(2):
            if method == 'request_timed_out': q.request_timed_out(None)
            else: q.response_rejected()
        if q.status == RequestReceipt.FAILED: q.response_received(b'late response')
        rows.append([state, method, q.status, q in q.link.pending_requests, q.failures])

packet = RNS.PacketReceipt.__new__(RNS.PacketReceipt)
packet.truncated_hash = bytes([7])*16
packet.status, packet.sent_at, packet.timeout = RNS.PacketReceipt.DELIVERED, time.time()-100, 1
packet.callbacks = SimpleNamespace(timeout=None, delivery=None)
link = SimpleNamespace(pending_requests=[])
q = RequestReceipt(link, packet_receipt=packet, timeout=1)
packet.check_timeout()
acked_status = q.status

resource_request = receipt(RequestReceipt.SENT)
resource_request.started_at, resource_request.timeout = None, 3600
resource_request.request_id = bytes([8])*16
resource_request.request_resource_concluded(SimpleNamespace(status=RNS.Resource.COMPLETE))
print(json.dumps({'guards':rows, 'acked':acked_status, 'resource':resource_request.status}))
"#;
    let output = std::process::Command::new(python)
        .args(["-c", script, &repo])
        .output()
        .expect("execute immutable reference guards");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("reference observations");
    let rows = value["guards"].as_array().expect("guard observations");
    assert_eq!(rows.len(), 6);
    for row in rows {
        let phase = row[0].as_u64().expect("initial phase");
        assert_eq!(row[2], if phase == 3 { 3 } else { 0 });
        assert_eq!(row[3], phase == 3);
        assert_eq!(row[4], if phase == 3 { 0 } else { 1 });
        let mut book = request_receipt::RequestBook::default();
        let link_id = AddressHash::new([1; ADDRESS_HASH_SIZE]);
        let hash = Hash::new([2; HASH_SIZE]);
        let request_id = hash.to_bytes()[..ADDRESS_HASH_SIZE].to_vec();
        let (receipt, mut dispatch) = book
            .register(
                link_id,
                request_id.clone(),
                RequestSource::Resource(hash),
                Some(Duration::from_secs(1)),
                None,
            )
            .expect("owned reference candidate");
        dispatch.confirm_handoff(true);
        dispatch.returned();
        if phase == 2 {
            book.request_resource_delivered(link_id, hash, Instant::now());
        }
        if phase == 3 {
            assert!(book.register_response_resource(link_id, &request_id, hash, hash, 1));
            book.response_progress(link_id, hash, 0, 1);
        }
        let reason = if row[1] == "request_timed_out" {
            RequestFailure::Timeout
        } else {
            RequestFailure::Rejected
        };
        assert_eq!(book.pending_failed(&request_id, reason), phase != 3);
        assert!(!book.pending_failed(&request_id, reason), "duplicate native callback is inert");
        assert_eq!(receipt.status() as u64, row[2].as_u64().expect("reference final status"));
        assert_eq!(
            book.pending_count() > 0,
            row[3].as_bool().expect("reference pending membership")
        );
    }
    assert_eq!(
        value["acked"], 1,
        "PacketReceipt ACK leaves RequestReceipt SENT and stops its packet timer"
    );
    assert_eq!(value["resource"], 2, "request Resource completion becomes DELIVERED");
}

#[tokio::test]
async fn signed_request_packet_ack_keeps_sent_stops_timer_and_cannot_reopen_rejection() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "request-ack"),
    };
    let events = tokio::sync::broadcast::channel(8).0;
    let mut local = Link::new(destination, events.clone());
    let request = local.request();
    let mut peer = Link::new_from_request(&request, signer.sign_key().clone(), destination, events)
        .expect("peer");
    let iface = AddressHash::new([3; ADDRESS_HASH_SIZE]);
    assert!(matches!(local.handle_packet(&peer.prove(), iface), LinkHandleResult::Activated));
    let request = Link::request_payload("/ack", rmpv::Value::Nil).expect("request");
    let packet = local.request_packet(&request.packed).expect("encrypted packet");
    let full_hash = packet.hash();
    let id = full_hash.to_bytes()[..ADDRESS_HASH_SIZE].to_vec();
    let (mut receipt, mut dispatch) = local
        .register_request(
            id.clone(),
            RequestSource::Packet(full_hash),
            Some(Duration::from_secs(1)),
            Some(1),
        )
        .expect("native request registration");
    dispatch.confirm_handoff(true);
    dispatch.returned();
    let proof = peer.prove_packet(&packet);
    local.handle_packet(&proof, iface);
    assert!(
        receipt.current().packet_acknowledged,
        "real peer signature proves the exact full request packet"
    );
    assert_eq!(receipt.status(), RequestStatus::Sent);
    local.requests.poll(Instant::now() + Duration::from_secs(10));
    assert_eq!(
        receipt.status(),
        RequestStatus::Sent,
        "ACK stops packet timeout without creating a response timer"
    );
    let oversized = peer
        .response_packet(&response_envelope(&id, &[1, 2, 3]))
        .expect("oversized response after signed ACK");
    local.handle_packet(&oversized, iface);
    assert_eq!(receipt.current().failure, Some(RequestFailure::Rejected));
    let terminal = receipt.current();
    local.handle_packet(&proof, iface);
    let response = peer.response_packet(&response_envelope(&id, b"late")).expect("late response");
    local.handle_packet(&response, iface);
    local.requests.request_resource_delivered(*local.id(), full_hash, Instant::now());
    assert_eq!(receipt.current(), terminal);
    assert_eq!(receipt.wait().await, Err(RequestFailure::Rejected));
    assert!(!local.take_response_limit_if_allowed(&id, 0));
    local.set_response_size_limit(&id, 1_000_000);
    local.clear_response_size_limit(&id);
    assert!(
        !local.take_response_limit_if_allowed(&id, 0),
        "legacy wrappers cannot erase a terminal ID"
    );
}

#[test]
fn request_deadlines_bind_full_packet_hash_and_resource_completion_without_restarting() {
    let link_id = AddressHash::new([31; ADDRESS_HASH_SIZE]);
    let hash = Hash::new([32; HASH_SIZE]);
    let id = hash.to_bytes()[..ADDRESS_HASH_SIZE].to_vec();
    for acknowledge in [false, true] {
        let mut book = request_receipt::RequestBook::default();
        let (receipt, mut dispatch) = book
            .register(
                link_id,
                id.clone(),
                RequestSource::Packet(hash),
                Some(Duration::from_secs(1)),
                None,
            )
            .expect("packet request");
        assert_eq!(book.next_deadline(), None, "registration is not handoff");
        dispatch.confirm_handoff(true);
        dispatch.returned();
        let sent = receipt.current().sent_at.expect("handoff clock");
        assert_eq!(book.next_deadline(), Some(sent + Duration::from_secs(1)));
        let mut wrong_hash = hash.to_bytes();
        wrong_hash[HASH_SIZE - 1] ^= 1;
        book.packet_acknowledged(link_id, Hash::new(wrong_hash));
        assert!(!receipt.current().packet_acknowledged, "a matching truncated ID is insufficient");
        if acknowledge {
            book.packet_acknowledged(link_id, hash);
            assert!(receipt.current().packet_acknowledged);
            assert_eq!(book.next_deadline(), None);
        }
        book.poll(sent + Duration::from_secs(1));
        assert_eq!(
            receipt.status(),
            RequestStatus::Sent,
            "reference timeout uses strict greater-than"
        );
        book.poll(sent + Duration::from_secs(2));
        assert_eq!(
            receipt.status(),
            if acknowledge { RequestStatus::Sent } else { RequestStatus::Failed }
        );
        if !acknowledge {
            assert_eq!(receipt.current().failure, Some(RequestFailure::Timeout));
        }
    }
    let mut book = request_receipt::RequestBook::default();
    let (receipt, mut dispatch) = book
        .register(
            link_id,
            id.clone(),
            RequestSource::Resource(hash),
            Some(Duration::from_secs(1)),
            None,
        )
        .expect("Resource request");
    dispatch.confirm_handoff(true);
    dispatch.returned();
    assert_eq!(book.next_deadline(), None, "Resource delivery retains its own retry owner");
    book.poll(Instant::now() + Duration::from_secs(1_000));
    assert_eq!(receipt.status(), RequestStatus::Sent);
    let delivered = Instant::now();
    book.request_resource_delivered(link_id, hash, delivered);
    assert_eq!(receipt.status(), RequestStatus::Delivered);
    assert_eq!(book.next_deadline(), Some(delivered + Duration::from_secs(1)));
    book.request_resource_delivered(link_id, hash, delivered + Duration::from_secs(10));
    assert_eq!(
        receipt.current().delivered_at,
        Some(delivered),
        "duplicate completion cannot extend the deadline"
    );
    let response_hash = Hash::new([33; HASH_SIZE]);
    assert!(book.register_response_resource(link_id, &id, response_hash, response_hash, 1_000));
    book.response_progress(link_id, response_hash, 100, 1_000);
    book.response_progress(link_id, response_hash, 0, 1_000);
    assert_eq!(
        receipt.current().received_bytes,
        100,
        "duplicate advertisement cannot rewind progress"
    );
    assert_eq!(book.next_deadline(), None);
    book.poll(delivered + Duration::from_secs(20));
    assert_eq!(receipt.status(), RequestStatus::Receiving);
}

#[test]
fn packet_response_limit_matches_reference_encoded_body_boundary_and_retains_terminal_fence() {
    for limit in [2, 3] {
        let mut book = request_receipt::RequestBook::default();
        let link_id = AddressHash::new([41; ADDRESS_HASH_SIZE]);
        let hash = Hash::new([42; HASH_SIZE]);
        let id = hash.to_bytes()[..ADDRESS_HASH_SIZE].to_vec();
        let (receipt, mut dispatch) = book
            .register(
                link_id,
                id.clone(),
                RequestSource::Packet(hash),
                Some(Duration::from_secs(1)),
                Some(limit),
            )
            .expect("owned bounded request");
        dispatch.confirm_handoff(true);
        dispatch.returned();
        let encoded = response_envelope(&id, &[1, 2, 3]);
        let (decoded_id, size) = response_envelope_metadata(&encoded).expect("strict envelope");
        assert_eq!(size, 3, "binary's two-byte msgpack prefix is excluded");
        assert_eq!(book.packet_response(link_id, &decoded_id, size, &encoded), limit == 3);
        assert_eq!(
            receipt.status(),
            if limit == 3 { RequestStatus::Ready } else { RequestStatus::Failed }
        );
        assert_eq!(
            receipt.current().packet_acknowledged,
            limit == 3,
            "an authenticated accepted response proves its child PacketReceipt"
        );
        let before = receipt.current();
        assert!(!book.packet_response(link_id, &id, 0, &response_envelope(&id, &[])));
        assert_eq!(receipt.current(), before);
    }
}

#[test]
fn explicit_cancel_retires_an_abandoned_registration_before_maintenance_without_rewriting_failure()
{
    let mut book = request_receipt::RequestBook::default();
    let link_id = AddressHash::new([51; ADDRESS_HASH_SIZE]);
    let hash = Hash::new([52; HASH_SIZE]);
    let id = hash.to_bytes()[..ADDRESS_HASH_SIZE].to_vec();
    let (receipt, guard) = book
        .register(link_id, id, RequestSource::Resource(hash), Some(Duration::from_secs(1)), None)
        .expect("native registration");
    drop(guard);
    let before = receipt.current();
    assert_eq!(before.failure, Some(RequestFailure::Cancelled));
    assert!(
        book.cancel(link_id, &receipt),
        "native retirement still has work after watch cancellation"
    );
    let cleanup = book.drain_cleanup();
    assert_eq!(cleanup.len(), 1);
    assert_eq!(cleanup[0].outbound, Some(hash));
    assert!(!book.cancel(link_id, &receipt));
    assert!(book.drain_cleanup().is_empty());
    assert_eq!(receipt.current(), before);
}
