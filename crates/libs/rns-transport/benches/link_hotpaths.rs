use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use rand_core::OsRng;
use rns_core::identity::PrivateIdentity as CorePrivateIdentity;
use rns_transport::crypt::fernet::{CachedFernet, Fernet, PlainText, Token};
use rns_transport::destination::link::{Link, LinkHandleResult};
use rns_transport::destination::{DestinationDesc, DestinationName};
use rns_transport::hash::{AddressHash, Hash};
use rns_transport::identity_bridge::to_transport_private_identity;
use rns_transport::iface::{IfaceSource, RxMessage, TxMessage, TxMessageType};
use rns_transport::packet::{
    DestinationType, Packet, PacketContext, PacketDataBuffer, PacketType, PACKET_MDU,
};
use rns_transport::resource::{
    build_link_packet, build_link_packet_into, ResourceManager, ResourceRequest,
};
use rns_transport::transport::interface_boundary::{
    InterfaceWorkerEnvelope, MAX_INTERFACE_WORKER_EVENT_BYTES,
};
use rns_transport::transport::worker_boundary::{
    decode_worker_frame, encode_worker_frame, WorkerJob, WorkerJobKind, WorkerRequest,
    WorkerResponse, WorkerResult, WorkerResultKind, MAX_WORKER_REQUEST_BYTES,
    MAX_WORKER_RESPONSE_BYTES,
};

const BURST_ITERS: usize = 64;

fn active_link_pair() -> (Link, Link, Vec<u8>) {
    let sender = CorePrivateIdentity::new_from_rand(OsRng);
    let receiver = CorePrivateIdentity::new_from_rand(OsRng);

    let _sender = to_transport_private_identity(&sender);
    let receiver = to_transport_private_identity(&receiver);

    let destination = DestinationDesc {
        identity: *receiver.as_identity(),
        address_hash: *receiver.address_hash(),
        name: DestinationName::new("lxmf", "delivery"),
    };

    let (tx, _) = tokio::sync::broadcast::channel(16);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();

    let mut inbound =
        Link::new_from_request(&request, receiver.sign_key().clone(), destination, tx)
            .expect("input link");
    let proof = inbound.prove();
    let proof_iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(outbound.handle_packet(&proof, proof_iface), LinkHandleResult::Activated));

    let payload = vec![0x2a; 128];
    (outbound, inbound, payload)
}

fn fernet_material() -> ([u8; 32], [u8; 32], Vec<u8>) {
    ([0x11; 32], [0x22; 32], vec![0x42; 128])
}

fn resource_request_fixture() -> (Link, Vec<u8>, ResourceRequest) {
    let (link, _, payload) = active_link_pair();
    let request = ResourceRequest {
        hashmap_exhausted: false,
        last_map_hash: None,
        resource_hash: Hash::new_from_slice(&[0x7a; 32]),
        requested_hashes: vec![[0x33; 4]; 8],
    };
    (link, payload, request)
}

fn decrypt_resource_packet(link: &Link, packet: &Packet) -> Packet {
    let mut plain_packet = *packet;
    let mut buffer = PacketDataBuffer::new();
    let plain_len = {
        let plaintext = link
            .decrypt(packet.data.as_slice(), buffer.accuire_buf_max())
            .expect("decrypt should succeed");
        plaintext.len()
    };
    buffer.resize(plain_len);
    plain_packet.data = buffer;
    plain_packet
}

fn resource_manager_request_fixture() -> (Link, ResourceManager, Packet) {
    let (sender_link, mut receiver_link, _) = active_link_pair();
    let mut sender_manager = ResourceManager::new();
    let mut receiver_manager = ResourceManager::new();
    let resource_data = vec![0x5a; PACKET_MDU * 6];

    let (resource_hash, advertisement_packet) = sender_manager
        .start_send(&sender_link, resource_data, None)
        .expect("resource send should succeed");
    sender_manager.confirm_outbound_dispatch(resource_hash, true);
    let plain_advertisement = decrypt_resource_packet(&receiver_link, &advertisement_packet);

    let mut responses = Vec::new();
    receiver_manager.handle_packet_into(&plain_advertisement, &mut receiver_link, &mut responses);
    let request_packet = responses.pop().expect("resource request packet");
    let plain_request = decrypt_resource_packet(&sender_link, &request_packet);

    (sender_link, sender_manager, plain_request)
}

fn worker_resource_complete_fixture() -> (WorkerRequest, WorkerResponse) {
    let resource_hash = [0x22; rns_transport::hash::HASH_SIZE];
    let proof = [0x33; rns_transport::hash::HASH_SIZE];
    let payload = vec![0x5a; PACKET_MDU * 6];
    let mut stream = vec![0x5a; rns_transport::resource::RANDOM_HASH_SIZE];
    stream.extend_from_slice(&payload);
    let request = WorkerRequest::new(
        WorkerJob {
            id: 0xfeed_beef,
            kind: WorkerJobKind::ResourceComplete {
                link_id: [0x11; rns_transport::hash::ADDRESS_HASH_SIZE],
                link_context: None,
                resource_hash,
                random_hash: [0x5a; rns_transport::resource::RANDOM_HASH_SIZE],
                encrypted: false,
                compressed: false,
                has_metadata: false,
                data_size: payload.len() as u64,
                request_id: None,
                is_request: false,
                is_response: false,
                stream: serde_bytes::ByteBuf::from(stream),
            },
        },
        1_000,
    );
    let response = WorkerResponse::success(WorkerResult {
        id: 0xfeed_beef,
        kind: WorkerResultKind::ResourceCompleted {
            resource_hash,
            proof,
            data: serde_bytes::ByteBuf::from(payload),
            metadata: None,
            request_id: None,
            is_request: false,
            is_response: false,
        },
    });
    (request, response)
}

fn interface_worker_packet(payload: &[u8]) -> Packet {
    let mut packet = Packet {
        destination: AddressHash::new([0xAB; rns_transport::hash::ADDRESS_HASH_SIZE]),
        ..Default::default()
    };
    packet.header.destination_type = DestinationType::Single;
    packet.header.packet_type = PacketType::Data;
    packet.context = PacketContext::None;
    packet.data = PacketDataBuffer::new_from_slice(payload);
    packet
}

fn interface_worker_envelope_fixture() -> (InterfaceWorkerEnvelope, InterfaceWorkerEnvelope) {
    let inbound = RxMessage {
        address: AddressHash::new([0x11; rns_transport::hash::ADDRESS_HASH_SIZE]),
        packet: interface_worker_packet(&vec![0x5a; 256]),
        source: IfaceSource::None,
    };
    let outbound = TxMessage {
        tx_type: TxMessageType::Direct(AddressHash::new(
            [0x22; rns_transport::hash::ADDRESS_HASH_SIZE],
        )),
        packet: interface_worker_packet(&vec![0xa5; 256]),
    };
    (
        InterfaceWorkerEnvelope::inbound_from_rx_message(1, &inbound)
            .expect("inbound interface envelope"),
        InterfaceWorkerEnvelope::outbound_from_tx_message(2, &outbound)
            .expect("outbound interface envelope"),
    )
}

fn bench_link_encrypt(c: &mut Criterion) {
    let (link, _, payload) = active_link_pair();
    let mut out = vec![0u8; PACKET_MDU + 256];
    c.bench_function("rns_transport/link_encrypt", |b| {
        b.iter(|| {
            let ciphertext = link
                .encrypt(black_box(&payload), black_box(out.as_mut_slice()))
                .expect("encrypt should succeed");
            black_box(ciphertext);
        });
    });
}

fn bench_link_decrypt(c: &mut Criterion) {
    let (outbound, inbound, payload) = active_link_pair();
    let mut cipher_buf = vec![0u8; PACKET_MDU + 256];
    let ciphertext = outbound
        .encrypt(&payload, cipher_buf.as_mut_slice())
        .expect("encrypt should succeed")
        .to_vec();
    let mut out = vec![0u8; PACKET_MDU + 256];
    c.bench_function("rns_transport/link_decrypt", |b| {
        b.iter(|| {
            let plaintext = inbound
                .decrypt(black_box(&ciphertext), black_box(out.as_mut_slice()))
                .expect("decrypt should succeed");
            black_box(plaintext);
        });
    });
}

fn bench_link_data_packet(c: &mut Criterion) {
    let (link, _, payload) = active_link_pair();
    c.bench_function("rns_transport/link_data_packet", |b| {
        b.iter(|| {
            let packet = link.data_packet(black_box(&payload)).expect("packet should succeed");
            black_box(packet);
        });
    });
}

fn bench_link_encrypt_burst(c: &mut Criterion) {
    let (link, _, payload) = active_link_pair();
    let mut out = vec![0u8; PACKET_MDU + 256];
    c.bench_function("rns_transport/link_encrypt_burst_64", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for _ in 0..BURST_ITERS {
                let ciphertext = link
                    .encrypt(black_box(&payload), black_box(out.as_mut_slice()))
                    .expect("encrypt should succeed");
                total += ciphertext.len();
            }
            black_box(total);
        });
    });
}

fn bench_link_decrypt_burst(c: &mut Criterion) {
    let (outbound, inbound, payload) = active_link_pair();
    let mut cipher_buf = vec![0u8; PACKET_MDU + 256];
    let ciphertext = outbound
        .encrypt(&payload, cipher_buf.as_mut_slice())
        .expect("encrypt should succeed")
        .to_vec();
    let mut out = vec![0u8; PACKET_MDU + 256];
    c.bench_function("rns_transport/link_decrypt_burst_64", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for _ in 0..BURST_ITERS {
                let plaintext = inbound
                    .decrypt(black_box(&ciphertext), black_box(out.as_mut_slice()))
                    .expect("decrypt should succeed");
                total += plaintext.len();
            }
            black_box(total);
        });
    });
}

fn bench_link_data_packet_burst(c: &mut Criterion) {
    let (link, _, payload) = active_link_pair();
    c.bench_function("rns_transport/link_data_packet_burst_64", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for _ in 0..BURST_ITERS {
                let packet = link.data_packet(black_box(&payload)).expect("packet should succeed");
                total += packet.data.len();
            }
            black_box(total);
        });
    });
}

fn bench_link_data_packet_reuse_burst(c: &mut Criterion) {
    let (link, _, payload) = active_link_pair();
    let mut packet = Packet::default();
    c.bench_function("rns_transport/link_data_packet_into_burst_64", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for _ in 0..BURST_ITERS {
                link.data_packet_into(black_box(&payload), black_box(&mut packet))
                    .expect("packet should succeed");
                total += packet.data.len();
            }
            black_box(total);
        });
    });
}

fn bench_resource_request_packet(c: &mut Criterion) {
    let (link, _, request) = resource_request_fixture();
    let payload = request.encode();
    c.bench_function("rns_transport/resource_request_packet", |b| {
        b.iter(|| {
            let packet = build_link_packet(
                &link,
                rns_transport::packet::PacketType::Data,
                rns_transport::packet::PacketContext::ResourceRequest,
                black_box(payload.as_slice()),
            )
            .expect("packet should succeed");
            black_box(packet);
        });
    });
}

fn bench_resource_request_packet_into(c: &mut Criterion) {
    let (link, _, request) = resource_request_fixture();
    let payload = request.encode();
    let mut packet = Packet::default();
    c.bench_function("rns_transport/resource_request_packet_into", |b| {
        b.iter(|| {
            build_link_packet_into(
                &link,
                rns_transport::packet::PacketType::Data,
                rns_transport::packet::PacketContext::ResourceRequest,
                black_box(payload.as_slice()),
                black_box(&mut packet),
            )
            .expect("packet should succeed");
            black_box(&packet);
        });
    });
}

fn bench_resource_part_packet_burst(c: &mut Criterion) {
    let (link, payload, _) = resource_request_fixture();
    c.bench_function("rns_transport/resource_part_packet_burst_64", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for _ in 0..BURST_ITERS {
                let packet = build_link_packet(
                    &link,
                    rns_transport::packet::PacketType::Data,
                    rns_transport::packet::PacketContext::Resource,
                    black_box(payload.as_slice()),
                )
                .expect("packet should succeed");
                total += packet.data.len();
            }
            black_box(total);
        });
    });
}

fn bench_resource_part_packet_into_burst(c: &mut Criterion) {
    let (link, payload, _) = resource_request_fixture();
    let mut packet = Packet::default();
    c.bench_function("rns_transport/resource_part_packet_into_burst_64", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for _ in 0..BURST_ITERS {
                build_link_packet_into(
                    &link,
                    rns_transport::packet::PacketType::Data,
                    rns_transport::packet::PacketContext::Resource,
                    black_box(payload.as_slice()),
                    black_box(&mut packet),
                )
                .expect("packet should succeed");
                total += packet.data.len();
            }
            black_box(total);
        });
    });
}

fn bench_resource_manager_request_window(c: &mut Criterion) {
    c.bench_function("rns_transport/resource_manager_request_window", |b| {
        b.iter(|| {
            let (mut sender_link, mut manager, plain_request) = resource_manager_request_fixture();
            let mut responses = Vec::new();
            manager.handle_packet_into(
                black_box(&plain_request),
                black_box(&mut sender_link),
                black_box(&mut responses),
            );
            black_box(responses.len());
        });
    });
}

fn bench_resource_manager_request_window_reuse(c: &mut Criterion) {
    let (mut sender_link, mut manager, plain_request) = resource_manager_request_fixture();
    let mut responses = Vec::new();
    c.bench_function("rns_transport/resource_manager_request_window_reuse", |b| {
        b.iter(|| {
            manager.handle_packet_into(
                black_box(&plain_request),
                black_box(&mut sender_link),
                black_box(&mut responses),
            );
            black_box(responses.len());
        });
    });
}

fn bench_resource_prepare_send(c: &mut Criterion) {
    let (link, _, _) = active_link_pair();
    let resource_data = vec![0x5a; PACKET_MDU * 6];
    c.bench_function("rns_transport/resource_prepare_send", |b| {
        b.iter_batched(
            || resource_data.clone(),
            |data| {
                let prepared = ResourceManager::prepare_send(
                    black_box(&link),
                    black_box(data),
                    black_box(None),
                )
                .expect("resource prepare should succeed");
                black_box(prepared);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_resource_worker_ipc_envelope(c: &mut Criterion) {
    let (request, response) = worker_resource_complete_fixture();
    let request_bytes = request.encode().expect("encode worker request");
    let response_bytes = response.encode().expect("encode worker response");
    let request_frame =
        encode_worker_frame(&request_bytes, MAX_WORKER_REQUEST_BYTES).expect("request frame");
    let response_frame =
        encode_worker_frame(&response_bytes, MAX_WORKER_RESPONSE_BYTES).expect("response frame");

    c.bench_function("rns_transport/resource_worker_ipc_envelope", |b| {
        b.iter(|| {
            let request_payload =
                decode_worker_frame(black_box(&request_frame), MAX_WORKER_REQUEST_BYTES)
                    .expect("decode request frame");
            let request =
                WorkerRequest::decode(black_box(request_payload)).expect("decode worker request");
            let encoded_request = request.encode().expect("re-encode worker request");
            let response_payload =
                decode_worker_frame(black_box(&response_frame), MAX_WORKER_RESPONSE_BYTES)
                    .expect("decode response frame");
            let response = WorkerResponse::decode(black_box(response_payload))
                .expect("decode worker response");
            let encoded_response = response.encode().expect("re-encode worker response");
            black_box((encoded_request.len(), encoded_response.len()));
        });
    });
}

fn bench_interface_worker_ipc_envelope(c: &mut Criterion) {
    let (inbound, outbound) = interface_worker_envelope_fixture();
    let inbound_bytes = inbound.encode().expect("encode inbound interface envelope");
    let outbound_bytes = outbound.encode().expect("encode outbound interface envelope");
    let inbound_frame = encode_worker_frame(&inbound_bytes, MAX_INTERFACE_WORKER_EVENT_BYTES)
        .expect("inbound interface frame");
    let outbound_frame = encode_worker_frame(&outbound_bytes, MAX_INTERFACE_WORKER_EVENT_BYTES)
        .expect("outbound interface frame");

    c.bench_function("rns_transport/interface_worker_ipc_envelope", |b| {
        b.iter(|| {
            let inbound_payload =
                decode_worker_frame(black_box(&inbound_frame), MAX_INTERFACE_WORKER_EVENT_BYTES)
                    .expect("decode inbound frame");
            let inbound = InterfaceWorkerEnvelope::decode(black_box(inbound_payload))
                .expect("decode inbound envelope");
            let encoded_inbound = inbound.encode().expect("re-encode inbound envelope");

            let outbound_payload =
                decode_worker_frame(black_box(&outbound_frame), MAX_INTERFACE_WORKER_EVENT_BYTES)
                    .expect("decode outbound frame");
            let outbound = InterfaceWorkerEnvelope::decode(black_box(outbound_payload))
                .expect("decode outbound envelope");
            let encoded_outbound = outbound.encode().expect("re-encode outbound envelope");

            black_box((encoded_inbound.len(), encoded_outbound.len()));
        });
    });
}

fn bench_fernet_encrypt_uncached(c: &mut Criterion) {
    let (sign_key, enc_key, payload) = fernet_material();
    let mut out = vec![0u8; PACKET_MDU];
    c.bench_function("rns_transport/fernet_encrypt_uncached", |b| {
        b.iter(|| {
            let token = Fernet::new_from_slices(&sign_key, &enc_key, OsRng)
                .encrypt(
                    PlainText::from(black_box(payload.as_slice())),
                    black_box(out.as_mut_slice()),
                )
                .expect("encrypt should succeed");
            black_box(token);
        });
    });
}

fn bench_fernet_encrypt_cached(c: &mut Criterion) {
    let (sign_key, enc_key, payload) = fernet_material();
    let cipher = CachedFernet::new_from_slices(&sign_key, &enc_key);
    let mut out = vec![0u8; PACKET_MDU];
    c.bench_function("rns_transport/fernet_encrypt_cached", |b| {
        b.iter(|| {
            let token = cipher
                .encrypt(
                    OsRng,
                    PlainText::from(black_box(payload.as_slice())),
                    black_box(out.as_mut_slice()),
                )
                .expect("encrypt should succeed");
            black_box(token);
        });
    });
}

fn bench_fernet_decrypt_uncached(c: &mut Criterion) {
    let (sign_key, enc_key, payload) = fernet_material();
    let token = {
        let mut cipher_buf = vec![0u8; PACKET_MDU];
        Fernet::new_from_slices(&sign_key, &enc_key, OsRng)
            .encrypt(PlainText::from(payload.as_slice()), cipher_buf.as_mut_slice())
            .expect("encrypt should succeed")
            .as_bytes()
            .to_vec()
    };
    let mut out = vec![0u8; PACKET_MDU];
    c.bench_function("rns_transport/fernet_decrypt_uncached", |b| {
        b.iter(|| {
            let verified = Fernet::new_from_slices(&sign_key, &enc_key, OsRng)
                .verify(Token::from(black_box(token.as_slice())))
                .expect("verify should succeed");
            let plaintext = Fernet::new_from_slices(&sign_key, &enc_key, OsRng)
                .decrypt(verified, black_box(out.as_mut_slice()))
                .expect("decrypt should succeed");
            black_box(plaintext);
        });
    });
}

fn bench_fernet_decrypt_cached(c: &mut Criterion) {
    let (sign_key, enc_key, payload) = fernet_material();
    let cipher = CachedFernet::new_from_slices(&sign_key, &enc_key);
    let token = {
        let mut cipher_buf = vec![0u8; PACKET_MDU];
        cipher
            .encrypt(OsRng, PlainText::from(payload.as_slice()), cipher_buf.as_mut_slice())
            .expect("encrypt should succeed")
            .as_bytes()
            .to_vec()
    };
    let mut out = vec![0u8; PACKET_MDU];
    c.bench_function("rns_transport/fernet_decrypt_cached", |b| {
        b.iter(|| {
            let verified = cipher
                .verify(Token::from(black_box(token.as_slice())))
                .expect("verify should succeed");
            let plaintext = cipher
                .decrypt(verified, black_box(out.as_mut_slice()))
                .expect("decrypt should succeed");
            black_box(plaintext);
        });
    });
}

criterion_group!(
    benches,
    bench_link_encrypt,
    bench_link_decrypt,
    bench_link_data_packet,
    bench_link_encrypt_burst,
    bench_link_decrypt_burst,
    bench_link_data_packet_burst,
    bench_link_data_packet_reuse_burst,
    bench_resource_request_packet,
    bench_resource_request_packet_into,
    bench_resource_part_packet_burst,
    bench_resource_part_packet_into_burst,
    bench_resource_manager_request_window,
    bench_resource_manager_request_window_reuse,
    bench_resource_prepare_send,
    bench_resource_worker_ipc_envelope,
    bench_interface_worker_ipc_envelope,
    bench_fernet_encrypt_uncached,
    bench_fernet_encrypt_cached,
    bench_fernet_decrypt_uncached,
    bench_fernet_decrypt_cached
);
criterion_main!(benches);
