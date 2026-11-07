//! Matched with `tools/scripts/python_impl_benchmarks.py`: Python's
//! `LXMessage.pack()` signs and `LXMessage.unpack_from_bytes()` validates the
//! signature, so encode benches sign and decode benches verify.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lxmf_core::inbound_decode::{decode_inbound_message, InboundPayloadMode};
use lxmf_core::message::WireMessage;
use lxmf_core::Message;
use rand_core::OsRng;
use rns_core::identity::{Identity, PrivateIdentity};

fn unsigned_message(title: &str, content: &str) -> Message {
    let mut message = Message::new();
    message.destination_hash = Some([0x11; 16]);
    message.source_hash = Some([0x22; 16]);
    message.timestamp = Some(1_770_000_000.0);
    message.set_title_from_string(title);
    message.set_content_from_string(content);
    message
}

fn verified(wire: &[u8], identity: &Identity) -> bool {
    WireMessage::unpack(wire)
        .expect("decode should succeed")
        .verify(identity)
        .expect("verify should succeed")
}

fn bench_decode(c: &mut Criterion, name: &str, title: &str, content: &str) {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let wire = unsigned_message(title, content)
        .to_wire(Some(&signer))
        .expect("sample message must encode");
    assert!(verified(&wire, &identity), "sample message must verify");
    c.bench_function(name, |b| {
        b.iter(|| black_box(verified(black_box(&wire), &identity)));
    });
}

fn bench_encode(c: &mut Criterion, name: &str, title: &str, content: &str) {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    c.bench_function(name, |b| {
        b.iter(|| {
            let message = unsigned_message(title, black_box(content));
            black_box(message.to_wire(Some(&signer)).expect("encode should succeed"));
        });
    });
}

fn bench_message_paths(c: &mut Criterion) {
    let large = "x".repeat(2048);
    let resource = "x".repeat(16_384);
    bench_decode(c, "lxmf_core/message_from_wire", "bench-title", "bench-content-payload");
    bench_encode(c, "lxmf_core/message_to_wire", "wire-title", "wire-content");
    bench_decode(c, "lxmf_core/large_message_from_wire", "bench-large-title", &large);
    bench_encode(c, "lxmf_core/large_message_to_wire", "wire-large-title", &large);
    bench_decode(c, "lxmf_core/resource_message_from_wire", "wire-resource-title", &resource);
    bench_encode(c, "lxmf_core/resource_message_to_wire", "wire-resource-title", &resource);
}

fn bench_decode_inbound_message(c: &mut Criterion) {
    let mut message = unsigned_message("bench-title", "bench-content-payload");
    message.signature = Some([0x33; 64]);
    let wire = message.to_wire(None).expect("sample message must encode");
    c.bench_function("lxmf_core/decode_inbound_message", |b| {
        b.iter(|| {
            let decoded = decode_inbound_message(
                black_box([0x11; 16]),
                black_box(&wire),
                InboundPayloadMode::FullWire,
            )
            .expect("inbound decode should succeed");
            black_box(decoded);
        });
    });
}

criterion_group!(benches, bench_message_paths, bench_decode_inbound_message);
criterion_main!(benches);
