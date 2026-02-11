use base64::Engine;
use lxmf::message::{Payload, WireMessage};
use reticulum::identity::PrivateIdentity;
use serde_bytes::ByteBuf;

#[test]
fn python_interop_gate() {
    if std::env::var("LXMF_PYTHON_INTEROP").ok().as_deref() != Some("1") {
        eprintln!("skipping python interop gate; set LXMF_PYTHON_INTEROP=1 to enable");
        return;
    }

    python_to_rust_wire_roundtrip();
    rust_to_python_wire_roundtrip();
}

fn python_to_rust_wire_roundtrip() {
    let output = std::process::Command::new("python3")
        .arg("tests/fixtures/python/lxmf/gen_live_interop_payload.py")
        .output()
        .expect("python3 must be executable");

    assert!(
        output.status.success(),
        "python interop script failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("interop json output");
    let wire_b64 = parsed
        .get("wire_b64")
        .and_then(serde_json::Value::as_str)
        .expect("wire_b64");
    let envelope_b64 = parsed
        .get("envelope_b64")
        .and_then(serde_json::Value::as_str)
        .expect("envelope_b64");
    let expected_content = parsed
        .get("content")
        .and_then(serde_json::Value::as_str)
        .expect("content")
        .as_bytes()
        .to_vec();
    let expected_title = parsed
        .get("title")
        .and_then(serde_json::Value::as_str)
        .expect("title")
        .as_bytes()
        .to_vec();

    let wire_bytes = base64::engine::general_purpose::STANDARD
        .decode(wire_b64)
        .expect("valid base64 wire");
    let envelope_bytes = base64::engine::general_purpose::STANDARD
        .decode(envelope_b64)
        .expect("valid base64 envelope");

    let message = WireMessage::unpack(&wire_bytes).expect("wire unpack");
    assert!(message.signature.is_some(), "python wire must be signed");
    assert_eq!(
        message.payload.content,
        Some(ByteBuf::from(expected_content))
    );
    assert_eq!(message.payload.title, Some(ByteBuf::from(expected_title)));
    assert!(message.payload.fields.is_some());

    let envelope = lxmf::propagation::unpack_envelope(&envelope_bytes).expect("envelope unpack");
    assert_eq!(envelope.messages.len(), 1);
    assert_eq!(envelope.messages[0], wire_bytes);
}

fn rust_to_python_wire_roundtrip() {
    let signer = PrivateIdentity::new_from_name("interop-rust-signer");
    let mut wire = WireMessage::new(
        [0x41; 16],
        [0x42; 16],
        Payload::new(
            1_700_000_000.0,
            Some(b"rust-content".to_vec()),
            Some(b"rust-title".to_vec()),
            None,
            None,
        ),
    );
    wire.sign(&signer).expect("sign");
    let packed = wire.pack().expect("pack");
    let packed_b64 = base64::engine::general_purpose::STANDARD.encode(packed);

    let output = std::process::Command::new("python3")
        .args([
            "-c",
            "import base64, json, sys\n\
import RNS.vendor.umsgpack as msgpack\n\
b = base64.b64decode(sys.argv[1])\n\
payload = msgpack.unpackb(b[96:])\n\
title = payload[1]\n\
content = payload[2]\n\
content = content.decode('utf-8') if isinstance(content, bytes) else content\n\
title = title.decode('utf-8') if isinstance(title, bytes) else title\n\
print(json.dumps({'content': content, 'title': title, 'signature_len': len(b[32:96])}))",
            &packed_b64,
        ])
        .output()
        .expect("python3 must be executable");

    assert!(
        output.status.success(),
        "python decode script failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("python decode output");
    assert_eq!(parsed["content"], "rust-content");
    assert_eq!(parsed["title"], "rust-title");
    assert_eq!(parsed["signature_len"], 64);
}
