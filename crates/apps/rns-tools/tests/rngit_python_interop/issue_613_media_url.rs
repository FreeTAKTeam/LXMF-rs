use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_media_decodes_encoded_path_and_returns_resource_metadata() -> io::Result<()> {
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
    let identity_seed = "rngit-python-media-url-server";
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
        let config_dir = temp.path().join("python-client");
        fs::create_dir_all(&config_dir)?;
        write_python_config(&config_dir, port)?;
        let identity = config_dir.join("identity");
        let output = Command::new(python_bin())
            .arg("-c")
            .arg(PYTHON_CLIENT)
            .arg(&config_dir)
            .arg(&identity)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python media URL client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let response: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                io::Error::other(format!(
                    "Python media URL client returned invalid JSON: {error}\nstdout:\n{}",
                    String::from_utf8_lossy(&output.stdout)
                ))
            })?;
        assert_eq!(response["encoded_path"], "/media/group/repo/HEAD/assets%2Fspace+name.bin");
        assert_eq!(response["name"], "space name.bin");
        assert_eq!(response["size"], 29);
        assert_eq!(
            response["sha256"],
            "78acd6db2006e4da7531327f95f5b00b97c53d18e59326e90dacdee2cba1e1a7"
        );
        assert_eq!(response["absent_blob_response_is_false"], true);
        assert_eq!(response["absent_blob_metadata_present"], false);
        assert_eq!(response["absent_blob_media_bytes_received"], false);
        Ok(())
    })();

    let _ = server.kill();
    let _ = server.wait();
    result
}

const PYTHON_CLIENT: &str = r#"
import hashlib
import json
import os
import sys
import threading
import RNS

config_dir, identity_path, destination_hex = sys.argv[1:4]
RNS.Reticulum(configdir=config_dir, loglevel=0)
identity = RNS.Identity.from_file(identity_path) if os.path.isfile(identity_path) else RNS.Identity()
if not os.path.isfile(identity_path):
    identity.to_file(identity_path)

destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
remote_identity = RNS.Identity.recall(destination_hash)
if remote_identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(
    remote_identity, RNS.Destination.OUT, RNS.Destination.SINGLE,
    "nomadnetwork", "node",
)
link_ready = threading.Event()
link_failed = []
def established(link):
    link.identify(identity)
    link_ready.set()
def closed(link):
    if not link_ready.is_set():
        link_failed.append("link closed before activation")
        link_ready.set()
link = RNS.Link(destination)
link.set_link_established_callback(established)
link.set_link_closed_callback(closed)
if not link_ready.wait(30):
    raise RuntimeError("rngit link establishment timed out")
if link_failed:
    raise RuntimeError(link_failed[0])

def request(path, data, timeout=30):
    finished = threading.Event()
    result = {}
    def response(receipt):
        value = receipt.response
        result["response_received"] = True
        result["metadata_present"] = receipt.metadata is not None
        if value is False:
            result["response_is_false"] = True
            result["media_bytes_received"] = False
            finished.set()
            return
        payload = value.read() if hasattr(value, "read") else value
        metadata = receipt.metadata or {}
        name = metadata.get("name", b"")
        result["name"] = name.decode("utf-8") if isinstance(name, bytes) else str(name)
        result["sha256"] = hashlib.sha256(payload).hexdigest()
        result["size"] = len(payload)
        result["media_bytes_received"] = bool(payload)
        finished.set()
    def failed(receipt):
        result["failed"] = True
        finished.set()
    receipt = link.request(path, data, response_callback=response,
                           failed_callback=failed, timeout=timeout)
    if receipt is False:
        result["failed"] = True
        return result
    if not finished.wait(timeout + 2):
        result["timed_out"] = True
    return result

encoded_path = "/media/group/repo/HEAD/assets%2Fspace+name.bin"
media = request("/media", {
    "key": b"rngit-percent-encoded-media-path",
    "path": encoded_path,
})
if "sha256" not in media:
    raise RuntimeError("encoded media request did not return a Resource")
missing_blob = request("/media", {
    "key": b"rngit-percent-encoded-media-path",
    "path": "/media/group/repo/HEAD/assets%2Fabsent+file.bin",
}, timeout=3)
link.teardown()
if not missing_blob.get("response_is_false"):
    raise RuntimeError("absent media blob did not return the reference False response")
print(json.dumps({
    "encoded_path": encoded_path,
    "name": media["name"],
    "sha256": media["sha256"],
    "size": media["size"],
    "absent_blob_response_is_false": missing_blob.get("response_is_false", False),
    "absent_blob_metadata_present": missing_blob.get("metadata_present", False),
    "absent_blob_media_bytes_received": missing_blob.get("media_bytes_received", True),
}, sort_keys=True))
"#;
