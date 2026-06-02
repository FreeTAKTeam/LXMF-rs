use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand_core::OsRng;
use rns_core::destination::{DestinationAnnounce, DestinationName, SingleInputDestination};
use rns_core::identity::{lxmf_sign, lxmf_verify, PrivateIdentity};
use rns_core::ratchets::{
    decrypt_with_identity_into, encrypt_for_public_key, encrypt_for_public_key_into,
};

const ANNOUNCE_BATCH_SIZE: usize = 64;

fn sample_destination() -> SingleInputDestination {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    SingleInputDestination::new(
        identity,
        DestinationName::new("example_utilities", "announcesample.fruits"),
    )
}

fn bench_announce_create(c: &mut Criterion) {
    let mut destination = sample_destination();
    c.bench_function("rns_core/announce_create", |b| {
        b.iter(|| {
            let packet = destination
                .announce(OsRng, black_box(Some(b"rust-announce-app-data".as_slice())))
                .expect("announce should succeed");
            black_box(packet);
        });
    });
}

fn bench_announce_validate(c: &mut Criterion) {
    let mut destination = sample_destination();
    let packet = destination
        .announce(OsRng, Some(b"rust-announce-app-data".as_slice()))
        .expect("announce should succeed");
    c.bench_function("rns_core/announce_validate", |b| {
        b.iter(|| {
            let info = DestinationAnnounce::validate(black_box(&packet))
                .expect("announce validation should succeed");
            black_box(info);
        });
    });
}

fn bench_announce_validate_batch_64(c: &mut Criterion) {
    let mut packets = Vec::with_capacity(ANNOUNCE_BATCH_SIZE);
    for index in 0..ANNOUNCE_BATCH_SIZE {
        let mut destination = sample_destination();
        let app_data = format!("rust-announce-app-data-{index}");
        let packet = destination
            .announce(OsRng, Some(app_data.as_bytes()))
            .expect("announce should succeed");
        packets.push(packet);
    }

    for packet in &packets {
        DestinationAnnounce::validate(packet).expect("batch announce fixture must validate");
    }

    c.bench_function("rns_core/announce_validate_batch_64", |b| {
        let mut signed_data = [0u8; rns_core::packet::PACKET_MDU];
        b.iter(|| {
            let mut validated = 0usize;
            for packet in &packets {
                let info = DestinationAnnounce::validate_with_buffer(
                    black_box(packet),
                    black_box(&mut signed_data),
                )
                .expect("announce validation should succeed");
                validated += info.app_data.len();
            }
            black_box(validated);
        });
    });
}

fn bench_identity_sign(c: &mut Criterion) {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let message = vec![0x5a; 2048];
    c.bench_function("rns_core/identity_sign", |b| {
        b.iter(|| {
            let signature = lxmf_sign(black_box(&identity), black_box(&message));
            black_box(signature);
        });
    });
}

fn bench_identity_verify(c: &mut Criterion) {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let public_identity = *identity.as_identity();
    let message = vec![0x5a; 2048];
    let signature = lxmf_sign(&identity, &message);
    assert!(lxmf_verify(&public_identity, &message, &signature), "signature fixture must verify");
    c.bench_function("rns_core/identity_verify", |b| {
        b.iter(|| {
            let valid = lxmf_verify(
                black_box(&public_identity),
                black_box(&message),
                black_box(&signature),
            );
            black_box(valid);
        });
    });
}

fn bench_identity_encrypt(c: &mut Criterion) {
    let recipient = PrivateIdentity::new_from_rand(OsRng);
    let public_identity = *recipient.as_identity();
    let plaintext = vec![0x42; 2048];
    let salt = public_identity.address_hash.as_slice().to_vec();
    let mut out = vec![0u8; 32 + plaintext.len() + 128];
    c.bench_function("rns_core/identity_encrypt", |b| {
        b.iter(|| {
            let ciphertext = encrypt_for_public_key_into(
                black_box(&public_identity.public_key),
                black_box(salt.as_slice()),
                black_box(&plaintext),
                black_box(out.as_mut_slice()),
                OsRng,
            )
            .expect("encryption should succeed");
            black_box(ciphertext);
        });
    });
}

fn bench_identity_decrypt(c: &mut Criterion) {
    let recipient = PrivateIdentity::new_from_rand(OsRng);
    let public_identity = *recipient.as_identity();
    let plaintext = vec![0x42; 2048];
    let salt = public_identity.address_hash.as_slice().to_vec();
    let ciphertext =
        encrypt_for_public_key(&public_identity.public_key, salt.as_slice(), &plaintext, OsRng)
            .expect("encryption should succeed");
    let mut out = vec![0u8; ciphertext.len()];
    c.bench_function("rns_core/identity_decrypt", |b| {
        b.iter(|| {
            let decrypted = decrypt_with_identity_into(
                black_box(&recipient),
                black_box(salt.as_slice()),
                black_box(&ciphertext),
                black_box(out.as_mut_slice()),
            )
            .expect("decryption should succeed");
            black_box(decrypted);
        });
    });
}

criterion_group!(
    benches,
    bench_announce_create,
    bench_announce_validate,
    bench_announce_validate_batch_64,
    bench_identity_sign,
    bench_identity_verify,
    bench_identity_encrypt,
    bench_identity_decrypt
);
criterion_main!(benches);
