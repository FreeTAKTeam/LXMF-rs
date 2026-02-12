use base64::Engine;
use lxmf::message::{Payload, WireMessage};
use rand_core::{CryptoRng, RngCore};
use reticulum::identity::PrivateIdentity;
use serde::Deserialize;
use serde_json::json;
use std::process::{Command, Stdio};

#[derive(Clone, Copy)]
struct FixedRng(u8);

impl RngCore for FixedRng {
    fn next_u32(&mut self) -> u32 {
        u32::from_le_bytes([self.0; 4])
    }

    fn next_u64(&mut self) -> u64 {
        u64::from_le_bytes([self.0; 8])
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        dest.fill(self.0);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for FixedRng {}

#[derive(Debug, Deserialize)]
struct GeneratePayload {
    source_private_b64: String,
    source_public_b64: String,
    source_hash_hex: String,
    destination_private_b64: String,
    destination_hash_hex: String,
    wire_b64: String,
    expected: ExpectedPayload,
}

#[derive(Debug, Deserialize)]
struct ExpectedPayload {
    title: String,
    content: String,
    attachment_names: Vec<String>,
    scope: String,
}

#[derive(Debug, Deserialize)]
struct VerifyPayload {
    wire: DecodedMessage,
    paper: DecodedMessage,
    propagation: DecodedMessage,
}

#[derive(Debug, Deserialize)]
struct DecodedMessage {
    title: Option<String>,
    content: Option<String>,
    signature_validated: bool,
    attachment_names: Vec<String>,
    scope: Option<String>,
}

#[test]
fn python_client_transport_interop_gate() {
    if std::env::var("LXMF_PYTHON_INTEROP").ok().as_deref() != Some("1") {
        eprintln!("skipping python client interop gate; set LXMF_PYTHON_INTEROP=1 to enable");
        return;
    }

    let generated = run_python_generate();

    let source_private = decode_b64(&generated.source_private_b64);
    let destination_private = decode_b64(&generated.destination_private_b64);
    let source_identity =
        PrivateIdentity::from_private_key_bytes(&source_private).expect("valid source identity");
    let destination_identity = PrivateIdentity::from_private_key_bytes(&destination_private)
        .expect("valid destination identity");

    let py_wire = decode_b64(&generated.wire_b64);
    let wire = WireMessage::unpack(&py_wire).expect("python wire unpack");
    assert_eq!(hex::encode(wire.source), generated.source_hash_hex);
    assert_eq!(hex::encode(wire.destination), generated.destination_hash_hex);
    assert!(wire.verify(source_identity.as_identity()).expect("python signature verify"));

    assert_eq!(decode_utf8(wire.payload.title.as_ref()), Some(generated.expected.title.clone()));
    assert_eq!(
        decode_utf8(wire.payload.content.as_ref()),
        Some(generated.expected.content.clone())
    );
    assert_eq!(attachment_names(wire.payload.fields.as_ref()), generated.expected.attachment_names);
    assert_eq!(
        scope_from_fields(wire.payload.fields.as_ref()),
        Some(generated.expected.scope.clone())
    );

    let rust_payload = Payload::new(
        1_700_000_321.0,
        Some(generated.expected.content.as_bytes().to_vec()),
        Some(generated.expected.title.as_bytes().to_vec()),
        Some(make_fields()),
        None,
    );

    let mut rust_wire = WireMessage::new(
        parse_hash16(&generated.destination_hash_hex),
        parse_hash16(&generated.source_hash_hex),
        rust_payload,
    );
    rust_wire.sign(&source_identity).expect("sign rust wire");
    let rust_wire_bytes = rust_wire.pack().expect("pack rust wire");
    let rust_paper = rust_wire
        .pack_paper_with_rng(destination_identity.as_identity(), FixedRng(0x7A))
        .expect("pack rust paper");
    let rust_propagation = rust_wire
        .pack_propagation_with_rng(
            destination_identity.as_identity(),
            1_700_000_321.0,
            FixedRng(0x7A),
        )
        .expect("pack rust propagation");

    let verify_input = json!({
        "source_hash_hex": generated.source_hash_hex,
        "source_public_b64": generated.source_public_b64,
        "destination_private_b64": generated.destination_private_b64,
        "wire_b64": base64::engine::general_purpose::STANDARD.encode(rust_wire_bytes),
        "paper_b64": base64::engine::general_purpose::STANDARD.encode(rust_paper),
        "propagation_b64": base64::engine::general_purpose::STANDARD.encode(rust_propagation),
    });
    let verified = run_python_verify(verify_input);

    assert_decoded(&verified.wire, &generated.expected);
    assert_decoded(&verified.paper, &generated.expected);
    assert_decoded(&verified.propagation, &generated.expected);
}

fn run_python_generate() -> GeneratePayload {
    let output = Command::new("python3")
        .args(["tests/fixtures/python/lxmf/live_transport_interop.py", "generate"])
        .output()
        .expect("python3 must be executable");

    assert!(
        output.status.success(),
        "python generate failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("valid generate json")
}

fn run_python_verify(input: serde_json::Value) -> VerifyPayload {
    let mut child = Command::new("python3")
        .args(["tests/fixtures/python/lxmf/live_transport_interop.py", "verify"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn python verify");

    let input_bytes = serde_json::to_vec(&input).expect("encode verify input");
    {
        let stdin = child.stdin.as_mut().expect("python verify stdin");
        use std::io::Write;
        stdin.write_all(&input_bytes).expect("write verify stdin");
    }

    let output = child.wait_with_output().expect("wait for python verify");
    assert!(
        output.status.success(),
        "python verify failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("valid verify json")
}

fn make_fields() -> rmpv::Value {
    rmpv::Value::Map(vec![
        (
            rmpv::Value::from("attachments"),
            rmpv::Value::Array(vec![rmpv::Value::Map(vec![
                (rmpv::Value::from("name"), rmpv::Value::from("note.txt")),
                (rmpv::Value::from("size"), rmpv::Value::from(5)),
                (rmpv::Value::from("media_type"), rmpv::Value::from("text/plain")),
                (rmpv::Value::from("hash"), rmpv::Value::from("sha256:deadbeef")),
            ])]),
        ),
        (
            rmpv::Value::from("_lxmf"),
            rmpv::Value::Map(vec![
                (rmpv::Value::from("scope"), rmpv::Value::from("chat")),
                (rmpv::Value::from("app"), rmpv::Value::from("weft")),
            ]),
        ),
        (
            rmpv::Value::from("announce"),
            rmpv::Value::Map(vec![
                (rmpv::Value::from("name"), rmpv::Value::from("node-alpha")),
                (rmpv::Value::from("stamp_cost"), rmpv::Value::from(20)),
            ]),
        ),
    ])
}

fn attachment_names(fields: Option<&rmpv::Value>) -> Vec<String> {
    let Some(rmpv::Value::Map(entries)) = fields else {
        return Vec::new();
    };
    let Some(rmpv::Value::Array(attachments)) = map_get(entries, "attachments") else {
        return Vec::new();
    };

    attachments
        .iter()
        .filter_map(|entry| {
            let rmpv::Value::Map(map) = entry else {
                return None;
            };
            match map_get(map, "name") {
                Some(rmpv::Value::String(name)) => name.as_str().map(str::to_string),
                _ => None,
            }
        })
        .collect()
}

fn scope_from_fields(fields: Option<&rmpv::Value>) -> Option<String> {
    let rmpv::Value::Map(entries) = fields? else {
        return None;
    };
    let rmpv::Value::Map(meta) = map_get(entries, "_lxmf")? else {
        return None;
    };
    match map_get(meta, "scope")? {
        rmpv::Value::String(scope) => scope.as_str().map(str::to_string),
        _ => None,
    }
}

fn map_get<'a>(entries: &'a [(rmpv::Value, rmpv::Value)], key: &str) -> Option<&'a rmpv::Value> {
    entries.iter().find_map(|(k, v)| match k {
        rmpv::Value::String(text) if text.as_str() == Some(key) => Some(v),
        _ => None,
    })
}

fn assert_decoded(decoded: &DecodedMessage, expected: &ExpectedPayload) {
    assert!(decoded.signature_validated, "signature should validate");
    assert_eq!(decoded.title.as_deref(), Some(expected.title.as_str()));
    assert_eq!(decoded.content.as_deref(), Some(expected.content.as_str()));
    assert_eq!(decoded.attachment_names, expected.attachment_names);
    assert_eq!(decoded.scope.as_deref(), Some(expected.scope.as_str()));
}

fn decode_b64(value: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(value).expect("valid base64")
}

fn decode_utf8(value: Option<&serde_bytes::ByteBuf>) -> Option<String> {
    value.and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
}

fn parse_hash16(hex_value: &str) -> [u8; 16] {
    let bytes = hex::decode(hex_value).expect("valid hash hex");
    assert_eq!(bytes.len(), 16, "expected 16-byte hash");
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    out
}
