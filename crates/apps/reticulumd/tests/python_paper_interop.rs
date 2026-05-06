use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use lxmf::{Payload, WireMessage};
use rand_core::OsRng;
use reticulum_daemon::lxmf_bridge::{
    build_wire_message, build_wire_message_with_options, decode_wire_message, rmpv_to_json,
};
use reticulum_daemon::lxmf_stamps::{
    generate_peering_key, generate_propagation_stamp, validate_peering_key,
    validate_propagation_stamp, validate_stamp, COST_TICKET,
};
use rns_core::identity::PrivateIdentity;
use sha2::{Digest, Sha256};

static PYTHON_PAPER_INTEROP_LOCK: Mutex<()> = Mutex::new(());

fn python_paper_interop_guard() -> MutexGuard<'static, ()> {
    PYTHON_PAPER_INTEROP_LOCK.lock().expect("paper interop lock")
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn python_paper_uri_decodes_in_rust() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let output = run_python_paper_helper(
        temp.path(),
        &["make-paper-uri", "--title", "Python Paper Title", "--content", "Python paper body"],
    );

    let recipient = private_identity_from_json_hex(&output, "recipient_private_key");
    let uri = output["uri"].as_str().expect("uri");
    let message = WireMessage::unpack_paper_uri(uri, &recipient).expect("decode Python paper URI");

    assert_eq!(message.destination, hex_16(&output, "recipient_hash"));
    assert_eq!(message.source, hex_16(&output, "source_hash"));
    assert_eq!(payload_string(message.payload.title.as_ref()), Some("Python Paper Title"));
    assert_eq!(payload_string(message.payload.content.as_ref()), Some("Python paper body"));
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn rust_paper_uri_ingests_in_python() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let setup = run_python_paper_helper(
        temp.path(),
        &["make-paper-uri", "--title", "setup", "--content", "setup"],
    );

    let recipient = private_identity_from_json_hex(&setup, "recipient_private_key");
    let signer = PrivateIdentity::new_from_name("rust-paper-sender");
    let destination = hex_16(&setup, "recipient_hash");
    let mut source = [0u8; 16];
    source.copy_from_slice(signer.address_hash().as_slice());

    let mut message = WireMessage::new(
        destination,
        source,
        Payload::new(
            1_777_777_777.0,
            Some(b"Rust paper body".to_vec()),
            Some(b"Rust Paper Title".to_vec()),
            None,
            None,
        ),
    );
    message.sign(&signer).expect("sign Rust paper message");
    let uri = message
        .pack_paper_uri_with_rng(recipient.as_identity(), OsRng)
        .expect("pack Rust paper URI");

    let recipient_private_key = setup["recipient_private_key"].as_str().expect("private key");
    let ingested = run_python_paper_helper(
        temp.path(),
        &["ingest-paper-uri", "--recipient-private-key", recipient_private_key, "--uri", &uri],
    );
    assert_eq!(ingested["result"], serde_json::json!("local"));

    let received = ingested["received"].as_array().expect("received array");
    assert_eq!(received.len(), 1);
    assert_eq!(received[0]["destination_hash"], setup["recipient_hash"]);
    assert_eq!(received[0]["source_hash"], serde_json::json!(hex::encode(source)));
    assert_eq!(received[0]["title"], serde_json::json!("Rust Paper Title"));
    assert_eq!(received[0]["content"], serde_json::json!("Rust paper body"));
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn rust_pow_stamp_validates_in_python() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let signer = PrivateIdentity::new_from_name("rust-pow-stamp-python-validation");
    let source = identity_hash_16(&signer);
    let destination = [0x51u8; 16];
    let wire = build_wire_message_with_options(
        source,
        destination,
        "stamp title",
        "stamp body",
        None,
        &signer,
        Some(1),
        None,
        None,
    )
    .expect("build stamped wire message");

    let validated = run_python_paper_helper(
        temp.path(),
        &["validate-wire-stamp", "--wire-hex", &hex::encode(wire), "--target-cost", "1"],
    );
    assert_eq!(validated["valid"], serde_json::json!(true));
    assert_eq!(validated["has_stamp"], serde_json::json!(true));
    assert!(validated["stamp_value"].as_u64().unwrap_or_default() >= 1);
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn rust_ticket_stamp_validates_in_python() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let signer = PrivateIdentity::new_from_name("rust-ticket-stamp-python-validation");
    let source = identity_hash_16(&signer);
    let destination = [0x61u8; 16];
    let ticket = [0xABu8; 16];
    let ticket_hex = hex::encode(ticket);
    let wire = build_wire_message_with_options(
        source,
        destination,
        "ticket title",
        "ticket body",
        None,
        &signer,
        None,
        Some(&ticket_hex),
        None,
    )
    .expect("build ticket-stamped wire message");

    let validated = run_python_paper_helper(
        temp.path(),
        &[
            "validate-wire-stamp",
            "--wire-hex",
            &hex::encode(wire),
            "--target-cost",
            &COST_TICKET.to_string(),
            "--ticket-hex",
            &ticket_hex,
        ],
    );
    assert_eq!(validated["valid"], serde_json::json!(true));
    assert_eq!(validated["has_stamp"], serde_json::json!(true));
    assert_eq!(validated["stamp_value"], serde_json::json!(COST_TICKET));
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn python_pow_stamp_validates_in_rust() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let stamped = run_python_paper_helper(
        temp.path(),
        &[
            "make-stamped-wire",
            "--title",
            "python pow stamp",
            "--content",
            "python pow body",
            "--target-cost",
            "1",
        ],
    );
    let wire = wire_from_json_hex(&stamped, "wire_hex");
    let stamp = wire.payload.stamp.as_ref().map(|value| value.as_ref());
    let value = validate_stamp(stamp, &wire.message_id(), 1, &[]).expect("valid Python stamp");

    assert_eq!(wire.destination, hex_16(&stamped, "destination_hash"));
    assert_eq!(wire.source, hex_16(&stamped, "source_hash"));
    assert!(value >= 1);
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn python_ticket_stamp_validates_in_rust() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let ticket = [0xCDu8; 16];
    let ticket_hex = hex::encode(ticket);
    let stamped = run_python_paper_helper(
        temp.path(),
        &[
            "make-stamped-wire",
            "--title",
            "python ticket stamp",
            "--content",
            "python ticket body",
            "--target-cost",
            &COST_TICKET.to_string(),
            "--ticket-hex",
            &ticket_hex,
        ],
    );
    let wire = wire_from_json_hex(&stamped, "wire_hex");
    let stamp = wire.payload.stamp.as_ref().map(|value| value.as_ref());
    let tickets = vec![ticket.to_vec()];
    let value = validate_stamp(stamp, &wire.message_id(), COST_TICKET, &tickets)
        .expect("valid Python ticket stamp");

    assert_eq!(wire.destination, hex_16(&stamped, "destination_hash"));
    assert_eq!(wire.source, hex_16(&stamped, "source_hash"));
    assert_eq!(value, COST_TICKET);
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn rust_propagation_stamp_validates_in_python() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let signer = PrivateIdentity::new_from_name("rust-pn-stamp-python-validation");
    let source = identity_hash_16(&signer);
    let destination = [0x71u8; 16];
    let wire = build_wire_message_with_options(
        source,
        destination,
        "rust pn stamp",
        "rust pn body",
        None,
        &signer,
        None,
        None,
        None,
    )
    .expect("build propagation wire message");
    let transient_id = sha256_array(&wire);
    let stamp = generate_propagation_stamp(&transient_id, 1).expect("generate propagation stamp");
    let mut transient = wire.clone();
    transient.extend_from_slice(&stamp);

    let validated = run_python_paper_helper(
        temp.path(),
        &["validate-pn-stamp", "--transient-hex", &hex::encode(transient), "--target-cost", "1"],
    );
    assert_eq!(validated["valid"], serde_json::json!(true));
    assert_eq!(validated["transient_id"], serde_json::json!(hex::encode(transient_id)));
    assert_eq!(validated["lxm_data_hex"], serde_json::json!(hex::encode(wire)));
    assert!(validated["stamp_value"].as_u64().unwrap_or_default() >= 1);
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn python_propagation_stamp_validates_in_rust() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let stamped = run_python_paper_helper(
        temp.path(),
        &[
            "make-pn-stamped-wire",
            "--title",
            "python pn stamp",
            "--content",
            "python pn body",
            "--target-cost",
            "1",
        ],
    );
    let transient =
        hex::decode(stamped["transient_hex"].as_str().expect("transient hex")).expect("hex");
    let value = validate_propagation_stamp(&transient, 1).expect("valid Python PN stamp");
    let wire = wire_from_json_hex(&stamped, "wire_hex");

    assert_eq!(
        stamped["transient_id"],
        serde_json::json!(hex::encode(sha256_array(&wire.pack().expect("repack wire"))))
    );
    assert!(value >= 1);
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn rust_attachment_fields_decode_in_python() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let signer = PrivateIdentity::new_from_name("rust-fields-python-decode");
    let source = identity_hash_16(&signer);
    let destination = [0x81u8; 16];
    let fields = serde_json::json!({
        "attachments": [
            {
                "name": "rust.bin",
                "data": [9, 8, 7],
            }
        ],
        "112": {"sender": "rust", "type": "field-test"}
    });
    let wire = build_wire_message(
        source,
        destination,
        "rust field title",
        "rust field body",
        Some(fields),
        &signer,
    )
    .expect("build Rust field wire");

    let inspected = run_python_paper_helper(
        temp.path(),
        &["inspect-wire-fields", "--wire-hex", &hex::encode(wire)],
    );

    assert_eq!(inspected["title"], serde_json::json!("rust field title"));
    assert_eq!(inspected["content"], serde_json::json!("rust field body"));
    assert_eq!(inspected["fields"]["5"], serde_json::json!([["rust.bin", [9, 8, 7]]]));
    assert_eq!(
        inspected["fields"]["112"],
        serde_json::json!({"sender": "rust", "type": "field-test"})
    );
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn python_attachment_fields_decode_in_rust() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let made = run_python_paper_helper(temp.path(), &["make-field-wire"]);
    let wire = hex::decode(made["wire_hex"].as_str().expect("wire hex")).expect("wire hex");
    let message = decode_wire_message(&wire).expect("decode Python field wire");
    let fields = message.fields.as_ref().and_then(rmpv_to_json).expect("fields to json");

    assert_eq!(message.title_as_string().as_deref(), Some("python field title"));
    assert_eq!(message.content_as_string().as_deref(), Some("python field body"));
    assert_eq!(fields["5"], serde_json::json!([["python.bin", [1, 2, 3]]]));
    assert_eq!(fields["112"], serde_json::json!({"sender": "python", "type": "field-test"}));
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn rust_peering_key_validates_in_python() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let peering_id = [0x91u8; 32];
    let key = generate_peering_key(&peering_id, 1).expect("generate peering key");

    let validated = run_python_paper_helper(
        temp.path(),
        &[
            "validate-peering-key",
            "--peering-id-hex",
            &hex::encode(peering_id),
            "--key-hex",
            &hex::encode(key),
            "--target-cost",
            "1",
        ],
    );

    assert_eq!(validated["valid"], serde_json::json!(true));
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF checkouts"]
fn python_peering_key_validates_in_rust() {
    let _guard = python_paper_interop_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let peering_id = [0xA1u8; 32];
    let made = run_python_paper_helper(
        temp.path(),
        &["make-peering-key", "--peering-id-hex", &hex::encode(peering_id), "--target-cost", "1"],
    );
    let key = hex::decode(made["key_hex"].as_str().expect("key hex")).expect("key hex");
    let value = validate_peering_key(&peering_id, &key, 1).expect("valid Python peering key");

    assert!(value >= 1);
    assert!(made["value"].as_u64().unwrap_or_default() >= 1);
}

fn run_python_paper_helper(temp_root: &Path, args: &[&str]) -> serde_json::Value {
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let lxmf_py_repo =
        env::var("LXMF_PY_REPO").map(PathBuf::from).unwrap_or_else(|_| repo_root.join("../lxmf"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_paper_endpoint.py");
    let python_path = format!("{}:{}", reticulum_py_repo.display(), lxmf_py_repo.display());

    let config_dir = temp_root.join("rns-config");
    let storage_dir = temp_root.join("lxmf-storage");
    let output = Command::new(python_bin)
        .arg(helper)
        .arg("--config-dir")
        .arg(config_dir)
        .arg("--storage-dir")
        .arg(storage_dir)
        .args(args)
        .env("PYTHONPATH", python_path)
        .output()
        .expect("run Python paper helper");

    if !output.status.success() {
        panic!(
            "Python paper helper failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8(output.stdout).expect("helper stdout utf8");
    let json_line = stdout.lines().rev().find(|line| !line.trim().is_empty()).expect("json line");
    serde_json::from_str(json_line).expect("helper json")
}

fn private_identity_from_json_hex(value: &serde_json::Value, key: &str) -> PrivateIdentity {
    let hex = value[key].as_str().expect("private key hex");
    let bytes = hex::decode(hex).expect("decode private key hex");
    PrivateIdentity::from_private_key_bytes(&bytes).expect("load private identity")
}

fn wire_from_json_hex(value: &serde_json::Value, key: &str) -> WireMessage {
    let bytes = hex::decode(value[key].as_str().expect("wire hex")).expect("decode wire hex");
    WireMessage::unpack(&bytes).expect("unpack wire")
}

fn sha256_array(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

fn identity_hash_16(identity: &PrivateIdentity) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(identity.address_hash().as_slice());
    out
}

fn hex_16(value: &serde_json::Value, key: &str) -> [u8; 16] {
    let bytes = hex::decode(value[key].as_str().expect("hex field")).expect("decode hex field");
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    out
}

fn payload_string<T: AsRef<[u8]>>(value: Option<&T>) -> Option<&str> {
    value.and_then(|bytes| std::str::from_utf8(bytes.as_ref()).ok())
}
