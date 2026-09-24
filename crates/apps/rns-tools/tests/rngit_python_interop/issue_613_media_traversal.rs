use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_media_denies_percent_decoded_dot_segment_over_python_link() -> io::Result<()> {
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
    let identity_seed = "rngit-python-media-dot-segment";
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
        let output = Command::new(python_bin())
            .arg("-c")
            .arg(PYTHON_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python media traversal client failed: {}\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| io::Error::other(format!("invalid client JSON: {error}")))?;
        assert_eq!(response["decoded_path"], "assets/../README.md");
        assert_eq!(response["response_received"], true);
        assert_eq!(response["failed"], false);
        assert_eq!(response["response_is_false"], true);
        assert_eq!(response["metadata_present"], false);
        assert_eq!(response["media_bytes_received"], false);
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
destination = RNS.Destination(remote_identity, RNS.Destination.OUT, RNS.Destination.SINGLE,
                              "nomadnetwork", "node")
ready = threading.Event()
link = RNS.Link(destination)
link.set_link_established_callback(lambda established_link: ready.set())
link.set_link_closed_callback(lambda closed_link: ready.set())
if not ready.wait(30) or not link.status:
    raise RuntimeError("media traversal Link did not establish")

completed = threading.Event()
result = {"decoded_path": "assets/../README.md", "response_received": False,
          "failed": False}
def response(receipt):
    result["response_received"] = True
    result["metadata_present"] = receipt.metadata is not None
    result["response_is_false"] = receipt.response is False
    result["media_bytes_received"] = False
    if receipt.response is not False:
        value = receipt.response.read() if hasattr(receipt.response, "read") else receipt.response
        result["media_bytes_received"] = bool(value)
    completed.set()
def failed(receipt):
    result["failed"] = True
    completed.set()

receipt = link.request("/media", {
    "key": b"rngit-percent-decoded-dot-segment",
    "path": "/media/group/repo/HEAD/assets%2F..%2FREADME.md",
}, response_callback=response, failed_callback=failed, timeout=10)
if receipt is False or not completed.wait(12):
    raise RuntimeError("media traversal request received no response")
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;
