use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_denies_private_media_over_unidentified_python_link() -> io::Result<()> {
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

    let source = temp.path().join("private-source");
    fs::create_dir_all(&source)?;
    run_git(&source, &["init", "-q"])?;
    run_git(&source, &["config", "user.email", "private-media@example.invalid"])?;
    run_git(&source, &["config", "user.name", "private-media-fixture"])?;
    fs::write(source.join("secret.bin"), b"PRIVATE_MEDIA_CANARY_613")?;
    run_git(&source, &["add", "secret.bin"])?;
    run_git(&source, &["commit", "-qm", "private media fixture"])?;
    let private_repo = root.join("private/repo");
    let private_repo_url = private_repo.to_string_lossy().into_owned();
    run_git(&source, &["remote", "add", "origin", &private_repo_url])?;
    run_git(&source, &["push", "-q", "origin", "HEAD:main"])?;

    let port = free_port()?;
    let identity_seed = "rngit-python-private-media";
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
                "Python private-media client failed: {}\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| io::Error::other(format!("invalid client JSON: {error}")))?;
        assert_eq!(response["denied"], true);
        assert_eq!(response["response_received"], true);
        assert_eq!(response["failed"], false);
        assert_eq!(response["response_is_false"], true);
        assert_eq!(response["metadata_present"], false);
        assert_eq!(response["media_bytes_received"], false);
        assert_eq!(response["private_canary_leaked"], false);
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

fn run_git(directory: &std::path::Path, args: &[&str]) -> io::Result<()> {
    let output = Command::new("git").args(args).current_dir(directory).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

const PYTHON_CLIENT: &str = r#"
import json
import os
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
def established(link): ready.set()
def closed(link):
    if not ready.is_set():
        closed_early.append(True)
        ready.set()
link = RNS.Link(destination)
link.set_link_established_callback(established)
link.set_link_closed_callback(closed)
if not ready.wait(30) or closed_early:
    raise RuntimeError("private-media Link did not establish")
finished = threading.Event()
result = {"response_received": False, "failed": False}
def response(receipt):
    value = receipt.response
    result["response_received"] = True
    result["metadata_present"] = receipt.metadata is not None
    if value is False:
        result["response_is_false"] = True
        result["media_bytes_received"] = False
        result["private_canary_leaked"] = False
    else:
        payload = value.read() if hasattr(value, "read") else value
        result["private_canary_leaked"] = b"PRIVATE_MEDIA_CANARY_613" in payload
        result["media_bytes_received"] = bool(payload)
    finished.set()
def failed(receipt):
    result["failed"] = True
    finished.set()
receipt = link.request("/media", {"key": b"private-media-check", "path": "/media/private/repo/HEAD/secret.bin"}, response_callback=response, failed_callback=failed, timeout=4)
if receipt is False:
    result["failed"] = True
else:
    finished.wait(6)
link.teardown()
result["denied"] = result.get("response_is_false", False) and not result.get("media_bytes_received", True)
result.setdefault("private_canary_leaked", False)
print(json.dumps(result, sort_keys=True))
"#;
