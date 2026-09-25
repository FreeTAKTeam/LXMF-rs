use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_media_temp_directory_creation_failure_returns_raw_resource() -> io::Result<()> {
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

    // A regular file at TMPDIR makes the production next_media_directory()
    // create_dir call fail deterministically without affecting the client.
    let invalid_temp_root = temp.path().join("not-a-directory");
    fs::write(&invalid_temp_root, b"block temp directory creation")?;
    let port = free_port()?;
    let identity_seed = "rngit-python-media-temp-directory-failure";
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
        .env("TMPDIR", &invalid_temp_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, identity_seed)?;
        let config_dir = temp.path().join("python-client");
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
                "Python media temp-directory-failure client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let response: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            io::Error::other(format!(
                "Python media temp-directory-failure client returned invalid JSON: {error}\nstdout:\n{}",
                String::from_utf8_lossy(&output.stdout)
            ))
        })?;
        assert_eq!(response["name"], "image.png", "response: {response}");
        assert_eq!(response["size"], 8192, "response: {response}");
        assert_eq!(
            response["sha256"], "f8e920545e99cdc9bbc2650eb8282344e8971a7ff0c397c91355d0fcaf6c61fa",
            "response: {response}"
        );
        assert!(invalid_temp_root.is_file(), "the injected filesystem fault must remain in place");
        Ok(())
    })();

    let _ = server.kill();
    let _ = server.wait();
    result
}

const PYTHON_CLIENT: &str = r#"
import hashlib
import json
import sys
import threading
import RNS

config_dir, destination_hex = sys.argv[1:3]
RNS.Reticulum(configdir=config_dir, loglevel=0)
destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
identity = RNS.Identity.recall(destination_hash)
if identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(
    identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node"
)
ready = threading.Event()
link = RNS.Link(destination)
link.set_link_established_callback(lambda _: ready.set())
link.set_link_closed_callback(lambda _: ready.set())
if not ready.wait(30) or link.status != RNS.Link.ACTIVE:
    raise RuntimeError("media temp-directory-failure Link did not establish")

finished = threading.Event()
result = {}
def response(receipt):
    payload = receipt.response.read()
    metadata = receipt.metadata or {}
    name = metadata.get("name", b"")
    result.update({
        "name": name.decode("utf-8") if isinstance(name, bytes) else str(name),
        "size": len(payload),
        "sha256": hashlib.sha256(payload).hexdigest(),
    })
    finished.set()
def failed(receipt):
    result["failed"] = str(receipt.status)
    finished.set()
link.request(
    "/media", {"key": b"present", "path": "/media/group/repo/HEAD/image.png"},
    response_callback=response, failed_callback=failed, timeout=30
)
if not finished.wait(35):
    raise RuntimeError("media temp-directory-failure response timed out")
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;
