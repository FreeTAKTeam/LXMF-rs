use std::process::Stdio;
use std::time::Duration;

use rand_core::OsRng;
use rns_rpc::rpc::control_boundary::{
    read_control_envelope, write_control_envelope, ControlEnvelope, ControlMessage,
};
use rns_rpc::RpcRequest;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::identity::PrivateIdentity;
use rns_transport::packet::{DestinationType, Header, Packet, PacketDataBuffer, PacketType};
use rns_transport::ratchets::encrypt_for_public_key_bytes;
use rns_transport::transport::interface_boundary::{
    write_interface_worker_envelope, InterfaceWorkerEnvelope, InterfaceWorkerEvent,
};
use rns_transport::transport::worker_boundary::{
    read_worker_frame, write_worker_frame, WorkerJob, WorkerJobKind, WorkerRequest, WorkerResponse,
    WorkerResultKind, MAX_WORKER_REQUEST_BYTES, MAX_WORKER_RESPONSE_BYTES,
};
use sha2::Digest;
use tokio::process::Command;
use tokio::time::timeout;

async fn submit_worker_request(request: WorkerRequest) -> WorkerResponse {
    let request = request.encode().expect("encode request");
    let mut child = Command::new(env!("CARGO_BIN_EXE_reticulumd"))
        .arg("--worker-stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker child process");
    let mut stdin = child.stdin.take().expect("worker stdin");
    let mut stdout = child.stdout.take().expect("worker stdout");

    write_worker_frame(&mut stdin, &request, MAX_WORKER_REQUEST_BYTES)
        .await
        .expect("write worker request");
    let response = read_worker_frame(&mut stdout, MAX_WORKER_RESPONSE_BYTES)
        .await
        .expect("read worker response");
    drop(stdin);

    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("worker child wait timeout")
        .expect("worker child wait");
    assert!(status.success(), "worker child exited with {status}");

    WorkerResponse::decode(&response).expect("decode worker response")
}

fn address_hash_bytes(hash: &rns_transport::hash::AddressHash) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(hash.as_slice());
    bytes
}

fn unencrypted_resource_complete_job(payload: &[u8]) -> WorkerJobKind {
    let random_hash = [0x5a; rns_transport::resource::RANDOM_HASH_SIZE];
    let mut hasher = sha2::Sha256::new();
    hasher.update(payload);
    hasher.update(random_hash);
    let digest = hasher.finalize();
    let mut resource_hash = [0u8; rns_transport::hash::HASH_SIZE];
    resource_hash.copy_from_slice(&digest[..rns_transport::hash::HASH_SIZE]);
    let mut stream = random_hash.to_vec();
    stream.extend_from_slice(payload);

    WorkerJobKind::ResourceComplete {
        link_id: [0x11; rns_transport::hash::ADDRESS_HASH_SIZE],
        link_context: None,
        resource_hash,
        random_hash,
        encrypted: false,
        compressed: false,
        has_metadata: false,
        data_size: payload.len() as u64,
        request_id: None,
        is_request: false,
        is_response: false,
        stream: serde_bytes::ByteBuf::from(stream),
    }
}

#[tokio::test]
async fn reticulumd_worker_stdio_validates_announce_in_child_process() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut destination =
        SingleInputDestination::new(identity, DestinationName::new("lxmf", "delivery"));
    let app_data = b"process worker announce";
    let announce = destination.announce(OsRng, Some(app_data)).expect("announce");
    let packet_wire = announce.to_bytes().expect("announce wire");
    let response = submit_worker_request(WorkerRequest::new(
        WorkerJob { id: 42, kind: WorkerJobKind::ValidateAnnounce { packet_wire } },
        1_000,
    ))
    .await;

    assert_eq!(response.job_id, 42);
    let result = response.outcome.expect("worker announce validation");
    assert_eq!(result.id, 42);
    let WorkerResultKind::AnnounceValidated {
        destination: validated_destination,
        public_key,
        verifying_key,
        name_hash,
        app_data: validated_app_data,
        ratchet,
    } = result.kind
    else {
        panic!("unexpected worker result kind");
    };
    let expected_destination = address_hash_bytes(&destination.desc.address_hash);
    let mut expected_name_hash = [0u8; rns_transport::destination::NAME_HASH_LENGTH];
    expected_name_hash.copy_from_slice(destination.desc.name.as_name_hash_slice());
    assert_eq!(validated_destination, expected_destination);
    assert_eq!(public_key, *destination.desc.identity.public_key.as_bytes());
    assert_eq!(verifying_key, *destination.desc.identity.verifying_key.as_bytes());
    assert_eq!(name_hash, expected_name_hash);
    assert_eq!(validated_app_data.as_ref(), app_data);
    assert!(ratchet.is_none());
}

#[tokio::test]
async fn reticulumd_worker_stdio_encrypts_outbound_in_child_process() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(identity, DestinationName::new("lxmf", "delivery"));
    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Single,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        destination: destination.desc.address_hash,
        data: PacketDataBuffer::new_from_slice(b"process outbound"),
        ..Default::default()
    };

    let response = submit_worker_request(WorkerRequest::new(
        WorkerJob {
            id: 43,
            kind: WorkerJobKind::OutboundEncrypt {
                packet_wire: packet.to_bytes().expect("packet wire"),
                public_key: *destination.desc.identity.public_key.as_bytes(),
                salt: address_hash_bytes(&destination.desc.identity.address_hash),
            },
        },
        1_000,
    ))
    .await;

    assert_eq!(response.job_id, 43);
    let result = response.outcome.expect("worker outbound encryption");
    let WorkerResultKind::PacketWire { packet_wire } = result.kind else {
        panic!("unexpected worker result kind");
    };
    let encrypted = Packet::from_bytes(&packet_wire).expect("encrypted packet");
    assert_eq!(encrypted.destination, packet.destination);
    assert_ne!(encrypted.data.as_slice(), b"process outbound");
}

#[tokio::test]
async fn reticulumd_worker_stdio_decrypts_single_destination_in_child_process() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(identity, DestinationName::new("lxmf", "delivery"));
    let salt = destination.identity.as_identity().address_hash;
    let ciphertext = encrypt_for_public_key_bytes(
        destination.desc.identity.public_key.as_bytes(),
        salt.as_slice(),
        b"process inbound",
        OsRng,
    )
    .expect("encrypt inbound");
    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Single,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        destination: destination.desc.address_hash,
        data: PacketDataBuffer::new_from_slice(&ciphertext),
        ..Default::default()
    };

    let response = submit_worker_request(WorkerRequest::new(
        WorkerJob {
            id: 44,
            kind: WorkerJobKind::SingleDestinationDecrypt {
                packet_wire: packet.to_bytes().expect("packet wire"),
                destination: address_hash_bytes(&destination.desc.address_hash),
                private_key: serde_bytes::ByteBuf::from(
                    destination.identity.to_private_key_bytes().to_vec(),
                ),
            },
        },
        1_000,
    ))
    .await;

    assert_eq!(response.job_id, 44);
    let result = response.outcome.expect("worker single destination decrypt");
    let WorkerResultKind::DestinationPayload { payload, ratchet_used } = result.kind else {
        panic!("unexpected worker result kind");
    };
    assert_eq!(payload.as_ref(), b"process inbound");
    assert!(!ratchet_used);
}

#[tokio::test]
async fn reticulumd_worker_stdio_completes_unencrypted_resource_in_child_process() {
    let response = submit_worker_request(WorkerRequest::new(
        WorkerJob { id: 45, kind: unencrypted_resource_complete_job(b"process resource payload") },
        1_000,
    ))
    .await;

    assert_eq!(response.job_id, 45);
    let result = response.outcome.expect("worker resource completion");
    let WorkerResultKind::ResourceCompleted {
        data,
        metadata,
        request_id,
        is_request,
        is_response,
        ..
    } = result.kind
    else {
        panic!("unexpected worker result kind");
    };
    assert_eq!(data.as_ref(), b"process resource payload");
    assert!(metadata.is_none());
    assert!(request_id.is_none());
    assert!(!is_request);
    assert!(!is_response);
}

#[tokio::test]
async fn reticulumd_worker_stdio_processes_multiple_jobs_in_one_child() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_reticulumd"))
        .arg("--worker-stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker child process");
    let mut stdin = child.stdin.take().expect("worker stdin");
    let mut stdout = child.stdout.take().expect("worker stdout");

    let first = WorkerRequest::new(
        WorkerJob { id: 46, kind: unencrypted_resource_complete_job(b"first payload") },
        1_000,
    )
    .encode()
    .expect("encode first request");
    let second = WorkerRequest::new(
        WorkerJob { id: 47, kind: unencrypted_resource_complete_job(b"second payload") },
        1_000,
    )
    .encode()
    .expect("encode second request");

    write_worker_frame(&mut stdin, &first, MAX_WORKER_REQUEST_BYTES)
        .await
        .expect("write first request");
    let first_response = read_worker_frame(&mut stdout, MAX_WORKER_RESPONSE_BYTES)
        .await
        .expect("read first response");
    write_worker_frame(&mut stdin, &second, MAX_WORKER_REQUEST_BYTES)
        .await
        .expect("write second request");
    let second_response = read_worker_frame(&mut stdout, MAX_WORKER_RESPONSE_BYTES)
        .await
        .expect("read second response");
    drop(stdin);

    let first_response = WorkerResponse::decode(&first_response).expect("decode first response");
    let second_response = WorkerResponse::decode(&second_response).expect("decode second response");
    assert_eq!(first_response.job_id, 46);
    assert_eq!(second_response.job_id, 47);
    let first = first_response.outcome.expect("first outcome");
    let second = second_response.outcome.expect("second outcome");
    let WorkerResultKind::ResourceCompleted { data: first_data, .. } = first.kind else {
        panic!("unexpected first result kind");
    };
    let WorkerResultKind::ResourceCompleted { data: second_data, .. } = second.kind else {
        panic!("unexpected second result kind");
    };
    assert_eq!(first_data.as_ref(), b"first payload");
    assert_eq!(second_data.as_ref(), b"second payload");

    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("worker child wait timeout")
        .expect("worker child wait");
    assert!(status.success(), "worker child exited with {status}");
}

#[tokio::test]
async fn reticulumd_interface_worker_stdio_accepts_framed_events_until_shutdown() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_reticulumd"))
        .arg("--interface-worker-stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn interface worker child process");
    let mut stdin = child.stdin.take().expect("interface worker stdin");
    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Single,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        destination: rns_transport::hash::AddressHash::new([0x44; 16]),
        data: PacketDataBuffer::new_from_slice(b"interface child event"),
        ..Default::default()
    };
    let event = InterfaceWorkerEnvelope::outbound_from_tx_message(
        1,
        &rns_transport::iface::TxMessage {
            tx_type: rns_transport::iface::TxMessageType::Direct(
                rns_transport::hash::AddressHash::new([0x55; 16]),
            ),
            packet,
        },
    )
    .expect("interface event");
    let shutdown = InterfaceWorkerEnvelope::new(2, InterfaceWorkerEvent::Shutdown);

    write_interface_worker_envelope(&mut stdin, &event).await.expect("write interface event");
    write_interface_worker_envelope(&mut stdin, &shutdown).await.expect("write interface shutdown");
    drop(stdin);

    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("interface worker child wait timeout")
        .expect("interface worker child wait");
    assert!(status.success(), "interface worker child exited with {status}");
}

#[tokio::test]
async fn reticulumd_control_router_stdio_serves_rpc_until_shutdown() {
    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let mut child = Command::new(env!("CARGO_BIN_EXE_reticulumd"))
        .arg("--control-router-stdio")
        .arg("--db")
        .arg(&db_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn control router child process");
    let mut stdin = child.stdin.take().expect("control router stdin");
    let mut stdout = child.stdout.take().expect("control router stdout");

    let request = ControlEnvelope::request(
        1,
        RpcRequest { id: 500, method: "daemon_status_ex".to_string(), params: None },
    );
    write_control_envelope(&mut stdin, &request).await.expect("write control request");
    let response = read_control_envelope(&mut stdout).await.expect("read control response");
    assert_eq!(response.sequence, 1);
    let ControlMessage::RpcResponse { response } = response.message else {
        panic!("expected rpc response");
    };
    assert_eq!(response.id, 500);
    assert!(response.error.is_none());
    assert!(response.result.is_some());

    write_control_envelope(&mut stdin, &ControlEnvelope::new(2, ControlMessage::Shutdown))
        .await
        .expect("write control shutdown");
    drop(stdin);

    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("control router child wait timeout")
        .expect("control router child wait");
    assert!(status.success(), "control router child exited with {status}");
}
