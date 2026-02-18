use rand_core::OsRng;
use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::identity::PrivateIdentity;
use serde::Deserialize;
use serde_json::json;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Debug, Deserialize)]
struct PythonAnnounceInteropReport {
    announce_valid: bool,
    announce_recalled: bool,
    path_response_valid: bool,
    path_response_recalled: bool,
    destination_hex: String,
}

#[test]
fn python_accepts_rust_announces_gate() {
    if std::env::var("LXMF_PYTHON_INTEROP").ok().as_deref() != Some("1") {
        eprintln!("skipping rust->python announce gate; set LXMF_PYTHON_INTEROP=1 to enable");
        return;
    }

    let identity = PrivateIdentity::new_from_name("rust-python-announce-gate");
    let mut destination =
        SingleInputDestination::new(identity, DestinationName::new("lxmf", "delivery"));

    let announce_packet = destination.announce(OsRng, None).expect("rust announce packet");
    let path_response_packet =
        destination.path_response(OsRng, None).expect("rust path response packet");

    let announce_raw = announce_packet.to_bytes().expect("announce bytes");
    let path_response_raw = path_response_packet.to_bytes().expect("path response bytes");
    let destination_hex = hex::encode(destination.desc.address_hash.as_slice());

    let input = json!({
        "announce_hex": hex::encode(announce_raw),
        "path_response_hex": hex::encode(path_response_raw),
    });

    let report = run_python_verify(&input);
    assert!(report.announce_valid, "python failed to validate rust announce");
    assert!(report.announce_recalled, "python did not cache identity from rust announce");
    assert!(report.path_response_valid, "python failed to validate rust path response");
    assert!(report.path_response_recalled, "python did not cache identity from rust path response");
    assert_eq!(
        report.destination_hex, destination_hex,
        "python parsed unexpected announce destination hash"
    );
}

fn run_python_verify(input: &serde_json::Value) -> PythonAnnounceInteropReport {
    let script = r#"
import json
import os
import sys
import tempfile

reticulum_path = os.environ.get("RETICULUM_PY_PATH")
if not reticulum_path:
    reticulum_path = os.path.abspath(os.path.join(os.getcwd(), "..", "Reticulum"))

sys.path.insert(0, reticulum_path)
import RNS  # noqa: E402

def _write_minimal_config(config_dir):
    os.makedirs(config_dir, exist_ok=True)
    with open(os.path.join(config_dir, "config"), "w", encoding="utf-8") as handle:
        handle.write(
            "\n".join(
                [
                    "[reticulum]",
                    "  enable_transport = False",
                    "  share_instance = No",
                    "  instance_name = rust-python-announce-gate",
                    "",
                    "[interfaces]",
                    "  [[Default Interface]]",
                    "    type = AutoInterface",
                    "    enabled = No",
                    "",
                ]
            )
        )

def _decode_packet(hex_blob):
    raw = bytes.fromhex(hex_blob)
    packet = RNS.Packet(None, raw)
    packet.unpack()
    return packet

with tempfile.TemporaryDirectory() as tmp:
    config_dir = os.path.join(tmp, ".reticulum")
    _write_minimal_config(config_dir)
    RNS.Reticulum(configdir=config_dir, loglevel=RNS.LOG_ERROR)

    payload = json.loads(sys.stdin.read())
    announce = _decode_packet(payload["announce_hex"])
    path_response = _decode_packet(payload["path_response_hex"])

    announce_valid = bool(RNS.Identity.validate_announce(announce))
    announce_recalled = RNS.Identity.recall(announce.destination_hash) is not None
    path_response_valid = bool(RNS.Identity.validate_announce(path_response))
    path_response_recalled = RNS.Identity.recall(path_response.destination_hash) is not None

    print(
        json.dumps(
            {
                "announce_valid": announce_valid,
                "announce_recalled": announce_recalled,
                "path_response_valid": path_response_valid,
                "path_response_recalled": path_response_recalled,
                "destination_hex": announce.destination_hash.hex(),
            }
        )
    )
"#;

    let mut child = Command::new("python3")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn python announce validator");

    let input_bytes = serde_json::to_vec(input).expect("encode python input");
    {
        let stdin = child.stdin.as_mut().expect("python stdin");
        stdin.write_all(&input_bytes).expect("write python stdin");
    }

    let output = child.wait_with_output().expect("python wait");
    assert!(
        output.status.success(),
        "python announce validation failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("decode python announce report")
}
