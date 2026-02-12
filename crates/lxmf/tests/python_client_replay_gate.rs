use base64::Engine;
use lxmf::message::WireMessage;
use rand_core::{CryptoRng, RngCore};
use reticulum::identity::PrivateIdentity;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
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
struct ReplayGeneratePayload {
    source_public_b64: String,
    source_hash_hex: String,
    destination_private_b64: String,
    vectors: Vec<ReplayVector>,
}

#[derive(Debug, Deserialize)]
struct ReplayVector {
    id: String,
    title: String,
    content: String,
    wire_b64: String,
    expected: ReplayExpected,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ReplayExpected {
    field_keys: Vec<i64>,
    attachment_names: Vec<String>,
    has_embedded_lxms: bool,
    has_image: bool,
    has_audio: bool,
    has_telemetry_stream: bool,
    has_thread: bool,
    has_results: bool,
    has_group: bool,
    has_event: bool,
    has_rnr_refs: bool,
    renderer: Option<i64>,
    commands_count: usize,
    has_telemetry: bool,
    has_ticket: bool,
    has_custom_type: bool,
    has_custom_data: bool,
    has_custom_meta: bool,
    has_non_specific: bool,
    has_debug: bool,
    command_ids: Vec<i64>,
    reply_to: Option<String>,
    reaction_to: Option<String>,
    reaction_emoji: Option<String>,
    reaction_sender: Option<String>,
    telemetry_location: Option<ReplayLocation>,
    capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReplayVerifyPayload {
    vectors: Vec<ReplayVerifiedVector>,
}

#[derive(Debug, Deserialize)]
struct ReplayVerifiedVector {
    id: String,
    wire: ReplayObserved,
    paper: ReplayObserved,
    propagation: ReplayObserved,
}

#[derive(Debug, Deserialize)]
struct ReplayObserved {
    title: Option<String>,
    content: Option<String>,
    signature_validated: bool,
    field_keys: Vec<i64>,
    attachment_names: Vec<String>,
    has_embedded_lxms: bool,
    has_image: bool,
    has_audio: bool,
    has_telemetry_stream: bool,
    has_thread: bool,
    has_results: bool,
    has_group: bool,
    has_event: bool,
    has_rnr_refs: bool,
    renderer: Option<i64>,
    commands_count: usize,
    has_telemetry: bool,
    has_ticket: bool,
    has_custom_type: bool,
    has_custom_data: bool,
    has_custom_meta: bool,
    has_non_specific: bool,
    has_debug: bool,
    command_ids: Vec<i64>,
    reply_to: Option<String>,
    reaction_to: Option<String>,
    reaction_emoji: Option<String>,
    reaction_sender: Option<String>,
    telemetry_location: Option<ReplayLocation>,
    capabilities: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct ReplayLocation {
    lat: f64,
    lon: f64,
    #[serde(default)]
    alt: Option<f64>,
}

#[test]
fn python_client_replay_gate() {
    if std::env::var("LXMF_PYTHON_INTEROP").ok().as_deref() != Some("1") {
        eprintln!("skipping python client replay gate; set LXMF_PYTHON_INTEROP=1 to enable");
        return;
    }

    let generated = run_python_generate();
    let source_public = decode_b64(&generated.source_public_b64);
    assert_eq!(source_public.len(), 64, "source public key must be 64 bytes");
    let source_identity =
        reticulum::identity::Identity::new_from_slices(&source_public[..32], &source_public[32..]);

    let destination_private = decode_b64(&generated.destination_private_b64);
    let destination_identity = PrivateIdentity::from_private_key_bytes(&destination_private)
        .expect("valid destination identity");

    let mut verify_vectors = Vec::new();
    for (index, vector) in generated.vectors.iter().enumerate() {
        let wire_bytes = decode_b64(&vector.wire_b64);
        let wire = WireMessage::unpack(&wire_bytes).expect("python wire unpack");
        assert_eq!(hex::encode(wire.source), generated.source_hash_hex);
        assert!(wire.verify(&source_identity).expect("python signature verifies"));

        assert_eq!(
            decode_utf8(wire.payload.title.as_ref()).as_deref(),
            Some(vector.title.as_str())
        );
        assert_eq!(
            decode_utf8(wire.payload.content.as_ref()).as_deref(),
            Some(vector.content.as_str())
        );
        assert_expected(&observed_from_wire(&wire), &vector.expected);

        let rng = FixedRng(0x40u8.wrapping_add(index as u8));
        let rust_wire = wire.pack().expect("re-pack rust wire");
        let rust_paper =
            wire.pack_paper_with_rng(destination_identity.as_identity(), rng).expect("rust paper");
        let rust_propagation = wire
            .pack_propagation_with_rng(
                destination_identity.as_identity(),
                1_700_002_000.0 + index as f64,
                rng,
            )
            .expect("rust propagation");

        verify_vectors.push(json!({
            "id": vector.id,
            "wire_b64": base64::engine::general_purpose::STANDARD.encode(rust_wire),
            "paper_b64": base64::engine::general_purpose::STANDARD.encode(rust_paper),
            "propagation_b64": base64::engine::general_purpose::STANDARD.encode(rust_propagation),
        }));
    }

    let verify_input = json!({
        "source_hash_hex": generated.source_hash_hex,
        "source_public_b64": generated.source_public_b64,
        "destination_private_b64": generated.destination_private_b64,
        "vectors": verify_vectors,
    });

    let verified = run_python_verify(verify_input);
    let expected_by_id: BTreeMap<_, _> =
        generated.vectors.iter().map(|vector| (vector.id.clone(), &vector.expected)).collect();
    let expected_text_by_id: BTreeMap<_, _> = generated
        .vectors
        .iter()
        .map(|vector| (vector.id.clone(), (vector.title.clone(), vector.content.clone())))
        .collect();

    for vector in &verified.vectors {
        let expected = expected_by_id.get(&vector.id).expect("expected vector id");
        let (title, content) = expected_text_by_id.get(&vector.id).expect("expected text id");
        assert_observed(&vector.wire, expected, title, content);
        assert_observed(&vector.paper, expected, title, content);
        assert_observed(&vector.propagation, expected, title, content);
    }
}

fn run_python_generate() -> ReplayGeneratePayload {
    let output = Command::new("python3")
        .args(["tests/fixtures/python/lxmf/live_client_replay.py", "generate"])
        .output()
        .expect("python3 must be executable");

    assert!(
        output.status.success(),
        "python replay generate failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("valid replay generate json")
}

fn run_python_verify(input: serde_json::Value) -> ReplayVerifyPayload {
    let mut child = Command::new("python3")
        .args(["tests/fixtures/python/lxmf/live_client_replay.py", "verify"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn python replay verify");

    let input_bytes = serde_json::to_vec(&input).expect("encode replay input");
    child
        .stdin
        .as_mut()
        .expect("python verify stdin")
        .write_all(&input_bytes)
        .expect("write verify stdin");

    let output = child.wait_with_output().expect("wait replay verify");
    assert!(
        output.status.success(),
        "python replay verify failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("valid replay verify json")
}

fn observed_from_wire(wire: &WireMessage) -> ReplayExpected {
    let fields = wire.payload.fields.as_ref();
    let extensions = field_value(fields, 16).and_then(extension_map);
    let reply_to =
        extensions.as_ref().and_then(|map| extension_string(map, &["reply_to", "replyTo"]));
    let reaction_to =
        extensions.as_ref().and_then(|map| extension_string(map, &["reaction_to", "reactionTo"]));
    let reaction_emoji = extensions
        .as_ref()
        .and_then(|map| extension_string(map, &["reaction_emoji", "reactionEmoji"]));
    let reaction_sender = extensions
        .as_ref()
        .and_then(|map| extension_string(map, &["reaction_sender", "reactionSender"]));

    ReplayExpected {
        field_keys: field_keys(fields),
        attachment_names: attachment_names(fields),
        has_embedded_lxms: field_value(fields, 1).is_some(),
        has_image: field_value(fields, 6).is_some(),
        has_audio: field_value(fields, 7).is_some(),
        has_telemetry_stream: field_value(fields, 3).is_some(),
        has_thread: field_value(fields, 8).is_some(),
        has_results: field_value(fields, 10).is_some(),
        has_group: field_value(fields, 11).is_some(),
        has_event: field_value(fields, 13).is_some(),
        has_rnr_refs: field_value(fields, 14).is_some(),
        renderer: field_value(fields, 15).and_then(value_to_i64),
        commands_count: field_value(fields, 9)
            .and_then(|value| match value {
                rmpv::Value::Array(items) => Some(items.len()),
                _ => None,
            })
            .unwrap_or(0),
        has_telemetry: field_value(fields, 2).is_some(),
        has_ticket: field_value(fields, 12).is_some(),
        has_custom_type: field_value(fields, 251).is_some(),
        has_custom_data: field_value(fields, 252).is_some(),
        has_custom_meta: field_value(fields, 253).is_some(),
        has_non_specific: field_value(fields, 254).is_some(),
        has_debug: field_value(fields, 255).is_some(),
        command_ids: command_ids(fields),
        reply_to,
        reaction_to,
        reaction_emoji,
        reaction_sender,
        telemetry_location: telemetry_location(fields),
        capabilities: extension_capabilities(extensions.as_deref()),
    }
}

fn field_keys(fields: Option<&rmpv::Value>) -> Vec<i64> {
    let Some(rmpv::Value::Map(entries)) = fields else {
        return Vec::new();
    };

    let mut keys: Vec<i64> = entries.iter().filter_map(|(key, _)| value_to_i64(key)).collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

fn field_value(fields: Option<&rmpv::Value>, target: i64) -> Option<&rmpv::Value> {
    let rmpv::Value::Map(entries) = fields? else {
        return None;
    };
    entries.iter().find_map(|(key, value)| {
        let key_i64 = value_to_i64(key)?;
        if key_i64 == target {
            Some(value)
        } else {
            None
        }
    })
}

fn value_to_i64(value: &rmpv::Value) -> Option<i64> {
    match value {
        rmpv::Value::Integer(int) => int.as_i64(),
        rmpv::Value::String(text) => text.as_str().and_then(|s| s.parse::<i64>().ok()),
        _ => None,
    }
}

fn attachment_names(fields: Option<&rmpv::Value>) -> Vec<String> {
    let Some(rmpv::Value::Array(attachments)) = field_value(fields, 5) else {
        return Vec::new();
    };

    attachments
        .iter()
        .filter_map(|attachment| match attachment {
            rmpv::Value::Array(items) if !items.is_empty() => value_to_string(&items[0]),
            _ => None,
        })
        .collect()
}

fn command_ids(fields: Option<&rmpv::Value>) -> Vec<i64> {
    let Some(rmpv::Value::Array(commands)) = field_value(fields, 9) else {
        return Vec::new();
    };

    let mut ids = Vec::new();
    for command in commands {
        let rmpv::Value::Map(entries) = command else {
            continue;
        };
        for (key, _) in entries {
            if let Some(id) = value_to_i64(key) {
                ids.push(id);
            }
        }
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn extension_map(value: &rmpv::Value) -> Option<Vec<(String, rmpv::Value)>> {
    let rmpv::Value::Map(entries) = value else {
        return None;
    };
    let mut out = Vec::new();
    for (key, value) in entries {
        let Some(key) = value_to_string(key) else {
            continue;
        };
        out.push((key, value.clone()));
    }
    Some(out)
}

fn extension_string(entries: &[(String, rmpv::Value)], keys: &[&str]) -> Option<String> {
    entries.iter().find_map(|(key, value)| {
        if keys.iter().any(|needle| needle == key) {
            value_to_string(value)
        } else {
            None
        }
    })
}

fn extension_capabilities(entries: Option<&[(String, rmpv::Value)]>) -> Vec<String> {
    let Some(entries) = entries else {
        return Vec::new();
    };
    let Some(value) =
        entries.iter().find_map(|(key, value)| (key == "capabilities").then_some(value))
    else {
        return Vec::new();
    };
    let rmpv::Value::Array(items) = value else {
        return Vec::new();
    };
    let mut capabilities: Vec<String> = items.iter().filter_map(value_to_string).collect();
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn telemetry_location(fields: Option<&rmpv::Value>) -> Option<ReplayLocation> {
    let telemetry = decode_telemetry_value(field_value(fields, 2)?)?;
    location_from_value(telemetry)
}

fn decode_telemetry_value(value: &rmpv::Value) -> Option<rmpv::Value> {
    match value {
        rmpv::Value::Binary(bytes) => {
            let mut cursor = std::io::Cursor::new(bytes);
            rmpv::decode::read_value(&mut cursor).ok()
        }
        other => Some(other.clone()),
    }
}

fn location_from_value(value: rmpv::Value) -> Option<ReplayLocation> {
    let rmpv::Value::Map(entries) = value else {
        return None;
    };

    let nested_location = map_lookup(&entries, &["location"]);
    if let Some(location) = nested_location {
        if let Some(parsed) = location_from_value(location.clone()) {
            return Some(parsed);
        }
    }

    let lat = map_lookup(&entries, &["lat", "latitude"]).and_then(value_to_f64)?;
    let lon = map_lookup(&entries, &["lon", "lng", "longitude"]).and_then(value_to_f64)?;
    let alt = map_lookup(&entries, &["alt", "altitude"]).and_then(value_to_f64);
    Some(ReplayLocation { lat, lon, alt })
}

fn map_lookup<'a>(
    entries: &'a [(rmpv::Value, rmpv::Value)],
    keys: &[&str],
) -> Option<&'a rmpv::Value> {
    entries.iter().find_map(|(key, value)| match key {
        rmpv::Value::String(text) => {
            let name = text.as_str()?;
            keys.iter().any(|needle| needle == &name).then_some(value)
        }
        _ => None,
    })
}

fn value_to_f64(value: &rmpv::Value) -> Option<f64> {
    match value {
        rmpv::Value::F32(v) => Some(f64::from(*v)),
        rmpv::Value::F64(v) => Some(*v),
        rmpv::Value::Integer(v) => {
            v.as_i64().map(|n| n as f64).or_else(|| v.as_u64().map(|n| n as f64))
        }
        rmpv::Value::String(text) => text.as_str().and_then(|s| s.parse::<f64>().ok()),
        _ => None,
    }
}

fn value_to_string(value: &rmpv::Value) -> Option<String> {
    match value {
        rmpv::Value::String(text) => text.as_str().map(str::to_string),
        rmpv::Value::Binary(bytes) => match String::from_utf8(bytes.clone()) {
            Ok(text) => Some(text),
            Err(_) => Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
        },
        _ => None,
    }
}

fn decode_utf8(value: Option<&serde_bytes::ByteBuf>) -> Option<String> {
    value.and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
}

fn assert_expected(observed: &ReplayExpected, expected: &ReplayExpected) {
    assert_eq!(observed.field_keys, expected.field_keys);
    assert_eq!(observed.attachment_names, expected.attachment_names);
    assert_eq!(observed.has_embedded_lxms, expected.has_embedded_lxms);
    assert_eq!(observed.has_image, expected.has_image);
    assert_eq!(observed.has_audio, expected.has_audio);
    assert_eq!(observed.has_telemetry_stream, expected.has_telemetry_stream);
    assert_eq!(observed.has_thread, expected.has_thread);
    assert_eq!(observed.has_results, expected.has_results);
    assert_eq!(observed.has_group, expected.has_group);
    assert_eq!(observed.has_event, expected.has_event);
    assert_eq!(observed.has_rnr_refs, expected.has_rnr_refs);
    assert_eq!(observed.renderer, expected.renderer);
    assert_eq!(observed.commands_count, expected.commands_count);
    assert_eq!(observed.has_telemetry, expected.has_telemetry);
    assert_eq!(observed.has_ticket, expected.has_ticket);
    assert_eq!(observed.has_custom_type, expected.has_custom_type);
    assert_eq!(observed.has_custom_data, expected.has_custom_data);
    assert_eq!(observed.has_custom_meta, expected.has_custom_meta);
    assert_eq!(observed.has_non_specific, expected.has_non_specific);
    assert_eq!(observed.has_debug, expected.has_debug);
    assert_eq!(observed.command_ids, expected.command_ids);
    assert_eq!(observed.reply_to, expected.reply_to);
    assert_eq!(observed.reaction_to, expected.reaction_to);
    assert_eq!(observed.reaction_emoji, expected.reaction_emoji);
    assert_eq!(observed.reaction_sender, expected.reaction_sender);
    assert_eq!(observed.telemetry_location, expected.telemetry_location);
    assert_eq!(observed.capabilities, expected.capabilities);
}

fn assert_observed(
    observed: &ReplayObserved,
    expected: &ReplayExpected,
    expected_title: &str,
    expected_content: &str,
) {
    assert!(observed.signature_validated, "signature should validate");
    assert_eq!(observed.title.as_deref(), Some(expected_title));
    assert_eq!(observed.content.as_deref(), Some(expected_content));
    assert_eq!(observed.field_keys, expected.field_keys);
    assert_eq!(observed.attachment_names, expected.attachment_names);
    assert_eq!(observed.has_embedded_lxms, expected.has_embedded_lxms);
    assert_eq!(observed.has_image, expected.has_image);
    assert_eq!(observed.has_audio, expected.has_audio);
    assert_eq!(observed.has_telemetry_stream, expected.has_telemetry_stream);
    assert_eq!(observed.has_thread, expected.has_thread);
    assert_eq!(observed.has_results, expected.has_results);
    assert_eq!(observed.has_group, expected.has_group);
    assert_eq!(observed.has_event, expected.has_event);
    assert_eq!(observed.has_rnr_refs, expected.has_rnr_refs);
    assert_eq!(observed.renderer, expected.renderer);
    assert_eq!(observed.commands_count, expected.commands_count);
    assert_eq!(observed.has_telemetry, expected.has_telemetry);
    assert_eq!(observed.has_ticket, expected.has_ticket);
    assert_eq!(observed.has_custom_type, expected.has_custom_type);
    assert_eq!(observed.has_custom_data, expected.has_custom_data);
    assert_eq!(observed.has_custom_meta, expected.has_custom_meta);
    assert_eq!(observed.has_non_specific, expected.has_non_specific);
    assert_eq!(observed.has_debug, expected.has_debug);
    assert_eq!(observed.command_ids, expected.command_ids);
    assert_eq!(observed.reply_to, expected.reply_to);
    assert_eq!(observed.reaction_to, expected.reaction_to);
    assert_eq!(observed.reaction_emoji, expected.reaction_emoji);
    assert_eq!(observed.reaction_sender, expected.reaction_sender);
    assert_eq!(observed.telemetry_location, expected.telemetry_location);
    assert_eq!(observed.capabilities, expected.capabilities);
}

fn decode_b64(value: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(value).expect("valid base64")
}
