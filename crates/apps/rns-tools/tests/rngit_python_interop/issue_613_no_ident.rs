use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_preserves_unblocked_anonymous_front_page_for_python_link() -> io::Result<()> {
    let _test_guard =
        super::PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let python_repo = python_repo();
    if !python_repo.join("RNS/Link.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }

    let port = free_port()?;
    let identity_seed = "rngit-python-no-ident-page";
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            identity_seed,
            "--silent",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, identity_seed)?;
        let config_dir = temp.path().join("python-unidentified-client");
        fs::create_dir_all(&config_dir)?;
        write_python_config(&config_dir, port)?;
        let output = Command::new(python_bin())
            .arg("-c")
            .arg(PYTHON_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python unidentified-page client failed: {}\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| io::Error::other(format!("invalid client JSON: {error}")))?;
        assert_eq!(response["response_received"], true, "{response}");
        assert_eq!(response["front_page"], true, "{response}");
        assert_eq!(response["group_visible"], true, "{response}");
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_returns_reference_no_ident_page_to_blocked_anonymous_python_link() -> io::Result<()> {
    const NULL_IDENTITY_HASH: &str = "d7db22f63b453c23bb0688dde565b7c1";
    let _test_guard =
        super::PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let python_repo = python_repo();
    if !python_repo.join("RNS/Link.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }

    let port = free_port()?;
    let identity_seed = "rngit-python-blocked-no-ident-page";
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            identity_seed,
            "--blocked-identity-hash",
            NULL_IDENTITY_HASH,
            "--silent",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, identity_seed)?;
        let config_dir = temp.path().join("python-blocked-anonymous-client");
        fs::create_dir_all(&config_dir)?;
        write_python_config(&config_dir, port)?;
        let output = Command::new(python_bin())
            .arg("-c")
            .arg(BLOCKED_PYTHON_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "pinned Python blocked-page client failed: {}\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| io::Error::other(format!("invalid client JSON: {error}")))?;
        assert_eq!(response["reference_null_identity_hash"], NULL_IDENTITY_HASH, "{response}");
        assert_eq!(response["request_status"], "READY", "{response}");
        let expected = format!(
            "#!c=0\n> Anonymous Git Node\n\n\n>>No Identity\n\nThis page requires identification, and none was received.\n\n<\n-\n`a`F666`[Served by rngit {}`:/page/index.mu] - local`f",
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(response["body"], expected, "unexpected no_ident response");
        assert!(!response["body"].as_str().unwrap_or_default().contains("private-canary"));
        assert!(!response["body"].as_str().unwrap_or_default().contains("secret-repository"));
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

const PYTHON_CLIENT: &str = r#"
import json
import sys
import threading
import RNS

config_dir, destination_hex = sys.argv[1:3]
RNS.Reticulum(configdir=config_dir, loglevel=0)
destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
remote_identity = RNS.Identity.recall(destination_hash)
if remote_identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(remote_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node")
ready = threading.Event()
closed_early = []
def established(link):
    ready.set()
def closed(link):
    if not ready.is_set():
        closed_early.append(True)
        ready.set()
link = RNS.Link(destination)
link.set_link_established_callback(established)
link.set_link_closed_callback(closed)
if not ready.wait(30) or closed_early:
    raise RuntimeError("unidentified page Link did not establish")
finished = threading.Event()
result = {"response_received": False}
def response(receipt):
    value = receipt.response
    payload = value.read() if hasattr(value, "read") else value
    result["response_received"] = True
    result["front_page"] = b"Groups" in payload and b"No Identity" not in payload
    result["group_visible"] = b"group" in payload
    finished.set()
def failed(receipt):
    result["request_failed"] = True
    finished.set()
receipt = link.request("/page/index.mu", {"var_g": "group"}, response_callback=response, failed_callback=failed, timeout=10)
if receipt is False or not finished.wait(15):
    raise RuntimeError("unidentified page request did not receive a response")
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;

const BLOCKED_PYTHON_CLIENT: &str = r#"
import json
import sys
import threading
import RNS

config_dir, destination_hex = sys.argv[1:3]
RNS.Reticulum(configdir=config_dir, loglevel=0)
reference_null_hash = RNS.Identity.from_bytes(bytes(64)).hash.hex()
if reference_null_hash != "d7db22f63b453c23bb0688dde565b7c1":
    raise RuntimeError("pinned Python null-identity hash changed: " + reference_null_hash)
destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
remote_identity = RNS.Identity.recall(destination_hash)
if remote_identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(remote_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node")
ready = threading.Event()
closed_early = []
def established(link):
    ready.set()
def closed(link):
    if not ready.is_set():
        closed_early.append(True)
        ready.set()
link = RNS.Link(destination)
link.set_link_established_callback(established)
link.set_link_closed_callback(closed)
if not ready.wait(30) or closed_early:
    raise RuntimeError("blocked anonymous page Link did not establish")
finished = threading.Event()
result = {"request_status": None, "body": None}
def response(receipt):
    result["request_status"] = "READY" if receipt.status == RNS.RequestReceipt.READY else receipt.status
    value = receipt.response
    result["body"] = value.decode("utf-8") if isinstance(value, bytes) else value.read().decode("utf-8")
    finished.set()
def failed(receipt):
    result["request_status"] = receipt.status
    finished.set()
receipt = link.request("/page/index.mu", {"var_g": "private"}, response_callback=response, failed_callback=failed, timeout=10)
if receipt is False or not finished.wait(15):
    raise RuntimeError("blocked anonymous page request did not conclude")
link.teardown()
result["reference_null_identity_hash"] = reference_null_hash
print(json.dumps(result, sort_keys=True))
"#;
