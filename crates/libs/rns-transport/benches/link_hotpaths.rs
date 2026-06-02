use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand_core::OsRng;
use rns_core::identity::PrivateIdentity as CorePrivateIdentity;
use rns_transport::crypt::fernet::{CachedFernet, Fernet, PlainText, Token};
use rns_transport::destination::link::{Link, LinkHandleResult};
use rns_transport::destination::{DestinationDesc, DestinationName};
use rns_transport::hash::{AddressHash, Hash};
use rns_transport::identity_bridge::to_transport_private_identity;
use rns_transport::packet::{Packet, PacketDataBuffer, PACKET_MDU};
use rns_transport::resource::{
    build_link_packet, build_link_packet_into, ResourceManager, ResourceRequest,
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

    let (_, advertisement_packet) = sender_manager
        .start_send(&sender_link, resource_data, None)
        .expect("resource send should succeed");
    let plain_advertisement = decrypt_resource_packet(&receiver_link, &advertisement_packet);

    let mut responses = Vec::new();
    receiver_manager.handle_packet_into(&plain_advertisement, &mut receiver_link, &mut responses);
    let request_packet = responses.pop().expect("resource request packet");
    let plain_request = decrypt_resource_packet(&sender_link, &request_packet);

    (sender_link, sender_manager, plain_request)
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
    bench_fernet_encrypt_uncached,
    bench_fernet_encrypt_cached,
    bench_fernet_decrypt_uncached,
    bench_fernet_decrypt_cached
);
criterion_main!(benches);
