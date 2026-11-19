//! RC1 fixture loading shared by the conformance and transfer tests.
#![allow(dead_code)]

use rpp_core::cbor::Value;
use rpp_core::{FileDraft, ManifestDraft, OwnerBinding, SigningIdentity, VerifiedManifest};
use serde_json::Value as Json;

pub fn fixtures() -> Json {
    serde_json::from_str(include_str!("../fixtures/rc1.json")).expect("rc1.json")
}

pub fn hex(value: &Json) -> Vec<u8> {
    hex::decode(value.as_str().expect("hex string")).expect("valid hex")
}

pub fn fixed<const N: usize>(bytes: Vec<u8>) -> [u8; N] {
    bytes.try_into().expect("fixed-size value")
}

/// JSON diagnostic notation → CBOR value (decimal keys, `h'..'` bytes).
pub fn diag(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Number(number) => Value::Uint(number.as_u64().expect("uint")),
        Json::String(text) => match text.strip_prefix("h'").and_then(|t| t.strip_suffix('\'')) {
            Some(bytes) => Value::Bytes(hex::decode(bytes).expect("diag hex")),
            None => Value::Text(text.clone()),
        },
        Json::Array(items) => Value::Array(items.iter().map(diag).collect()),
        Json::Object(entries) => Value::Map(
            entries.iter().map(|(k, v)| (k.parse().expect("uint key"), diag(v))).collect(),
        ),
        Json::Bool(_) => panic!("booleans are not used by RPP"),
    }
}

pub fn identity(fixtures: &Json, key: &str) -> SigningIdentity {
    SigningIdentity::from_private_bytes(&fixed(hex(&fixtures[key])))
}

pub fn package_owner(fixtures: &Json, name: &str) -> SigningIdentity {
    let public = hex(&fixtures["packages"][name]["publisher_public_hex"]);
    for key in ["fixture_identity_private_hex", "second_fixture_identity_private_hex"] {
        let candidate = identity(fixtures, key);
        if candidate.public().0.as_slice() == public.as_slice() {
            return candidate;
        }
    }
    panic!("no fixture identity for package {name}");
}

pub fn binding(fixtures: &Json, name: &str) -> OwnerBinding {
    let value = &fixtures["packages"][name]["value"];
    OwnerBinding {
        owner: package_owner(fixtures, name).public(),
        package_destination: fixed(diag_bytes(&value["8"])),
        manifest_destination: fixed(diag_bytes(&value["9"])),
    }
}

pub fn diag_bytes(value: &Json) -> Vec<u8> {
    match diag(value) {
        Value::Bytes(bytes) => bytes,
        other => panic!("expected bytes, got {other:?}"),
    }
}

/// Fixture file contents: (path, chunk byte strings).
pub fn package_files(fixtures: &Json, name: &str) -> Vec<(String, Vec<Vec<u8>>)> {
    fixtures["packages"][name]["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|file| {
            let chunks = file["chunks_hex"].as_array().expect("chunks").iter().map(hex).collect();
            (file["path"].as_str().expect("path").to_owned(), chunks)
        })
        .collect()
}

/// Rebuilds a fixture package from its files and fixture metadata.
pub fn rebuild(fixtures: &Json, name: &str) -> VerifiedManifest {
    let value = &fixtures["packages"][name]["value"];
    let binding = binding(fixtures, name);
    let files = package_files(fixtures, name)
        .into_iter()
        .zip(value["31"].as_array().expect("file descriptors"))
        .map(|((path, chunks), descriptor)| {
            let refs: Vec<&[u8]> = chunks.iter().map(Vec::as_slice).collect();
            FileDraft::from_chunks(path, fixed(diag_bytes(&descriptor["46"])), &refs)
        })
        .collect();
    let draft = ManifestDraft {
        package_destination: binding.package_destination,
        manifest_destination: binding.manifest_destination,
        files,
        predecessors: value["14"]
            .as_array()
            .map(|list| list.iter().map(|d| fixed(diag_bytes(d))).collect())
            .unwrap_or_default(),
        sequence: value["15"].as_u64(),
        ..ManifestDraft::default()
    };
    draft.sign(&package_owner(fixtures, name)).expect("fixture manifest signs")
}
