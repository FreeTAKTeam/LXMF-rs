use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use lxmf::WireMessage;
use rns_core::identity::PrivateIdentity;
use sha2::{Digest, Sha256};

static PYTHON_PAPER_INTEROP_LOCK: Mutex<()> = Mutex::new(());

pub(super) fn python_paper_interop_guard() -> MutexGuard<'static, ()> {
    PYTHON_PAPER_INTEROP_LOCK.lock().expect("paper interop lock")
}

pub(super) fn run_python_paper_helper(temp_root: &Path, args: &[&str]) -> serde_json::Value {
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

pub(super) fn private_identity_from_json_hex(
    value: &serde_json::Value,
    key: &str,
) -> PrivateIdentity {
    let hex = value[key].as_str().expect("private key hex");
    let bytes = hex::decode(hex).expect("decode private key hex");
    PrivateIdentity::from_private_key_bytes(&bytes).expect("load private identity")
}

pub(super) fn wire_from_json_hex(value: &serde_json::Value, key: &str) -> WireMessage {
    let bytes = hex::decode(value[key].as_str().expect("wire hex")).expect("decode wire hex");
    WireMessage::unpack(&bytes).expect("unpack wire")
}

pub(super) fn sha256_array(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

pub(super) fn identity_hash_16(identity: &PrivateIdentity) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(identity.address_hash().as_slice());
    out
}

pub(super) fn hex_16(value: &serde_json::Value, key: &str) -> [u8; 16] {
    let bytes = hex::decode(value[key].as_str().expect("hex field")).expect("decode hex field");
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    out
}

pub(super) fn payload_string<T: AsRef<[u8]>>(value: Option<&T>) -> Option<&str> {
    value.and_then(|bytes| std::str::from_utf8(bytes.as_ref()).ok())
}
