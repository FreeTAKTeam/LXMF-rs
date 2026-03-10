use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lxmf_core::inbound_decode::{decode_inbound_message, InboundPayloadMode};
use lxmf_core::Message;

fn sample_wire_payload() -> (Vec<u8>, [u8; 16]) {
    let mut message = Message::new();
    let destination = [0x11; 16];
    let source = [0x22; 16];
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.signature = Some([0x33; 64]);
    message.timestamp = Some(1_770_000_000.0);
    message.set_title_from_string("bench-title");
    message.set_content_from_string("bench-content-payload");
    let wire = message.to_wire(None).expect("sample message must encode");
    (wire, destination)
}

fn sample_large_wire_payload() -> (Vec<u8>, [u8; 16]) {
    let mut message = Message::new();
    let destination = [0x77; 16];
    let source = [0x88; 16];
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.signature = Some([0x99; 64]);
    message.timestamp = Some(1_770_000_100.0);
    message.set_title_from_string("bench-large-title");
    message.set_content_from_string(&"x".repeat(2048));
    let wire = message.to_wire(None).expect("large sample message must encode");
    (wire, destination)
}

fn bench_message_from_wire(c: &mut Criterion) {
    let (wire, _) = sample_wire_payload();
    c.bench_function("lxmf_core/message_from_wire", |b| {
        b.iter(|| {
            let decoded = Message::from_wire(black_box(&wire)).expect("decode should succeed");
            black_box(decoded);
        });
    });
}

fn bench_decode_inbound_message(c: &mut Criterion) {
    let (wire, fallback_destination) = sample_wire_payload();
    c.bench_function("lxmf_core/decode_inbound_message", |b| {
        b.iter(|| {
            let decoded = decode_inbound_message(
                black_box(fallback_destination),
                black_box(&wire),
                InboundPayloadMode::FullWire,
            )
            .expect("inbound decode should succeed");
            black_box(decoded);
        });
    });
}

fn bench_message_to_wire(c: &mut Criterion) {
    c.bench_function("lxmf_core/message_to_wire", |b| {
        b.iter(|| {
            let mut message = Message::new();
            message.destination_hash = Some([0x44; 16]);
            message.source_hash = Some([0x55; 16]);
            message.signature = Some([0x66; 64]);
            message.timestamp = Some(1_770_000_001.0);
            message.set_title_from_string("wire-title");
            message.set_content_from_string("wire-content");
            let wire = message.to_wire(None).expect("encode should succeed");
            black_box(wire);
        });
    });
}

fn bench_large_message_from_wire(c: &mut Criterion) {
    let (wire, _) = sample_large_wire_payload();
    c.bench_function("lxmf_core/large_message_from_wire", |b| {
        b.iter(|| {
            let decoded = Message::from_wire(black_box(&wire)).expect("decode should succeed");
            black_box(decoded);
        });
    });
}

fn bench_large_message_to_wire(c: &mut Criterion) {
    let content = "x".repeat(2048);
    c.bench_function("lxmf_core/large_message_to_wire", |b| {
        b.iter(|| {
            let mut message = Message::new();
            message.destination_hash = Some([0xa4; 16]);
            message.source_hash = Some([0xb5; 16]);
            message.signature = Some([0xc6; 64]);
            message.timestamp = Some(1_770_000_101.0);
            message.set_title_from_string("wire-large-title");
            message.set_content_from_string(black_box(&content));
            let wire = message.to_wire(None).expect("encode should succeed");
            black_box(wire);
        });
    });
}

criterion_group!(
    benches,
    bench_message_from_wire,
    bench_decode_inbound_message,
    bench_message_to_wire,
    bench_large_message_from_wire,
    bench_large_message_to_wire
);
criterion_main!(benches);
