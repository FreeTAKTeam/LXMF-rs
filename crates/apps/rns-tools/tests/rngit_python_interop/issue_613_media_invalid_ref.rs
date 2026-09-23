use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_media_validation_denials_return_false_over_python_link() -> io::Result<()> {
    let _test_guard =
        super::PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    seed_private_media_repository(&root, &temp.path().join("private-source"))?;
    let python_repo = python_repo();
    if !python_repo.join("RNS/Link.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }

    let port = free_port()?;
    let identity_seed = "rngit-python-invalid-media-ref";
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
                "Python invalid-ref client failed: {}\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let result: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| io::Error::other(format!("invalid client JSON: {error}")))?;
        assert_eq!(result["valid_ref"]["name"], "space name.bin", "client result: {result}");
        assert_eq!(
            result["valid_ref"]["payload_hex"],
            "70657263656e74206465636f646564206d65646961207061746800ff0a"
        );
        assert_eq!(result["valid_ref"]["metadata_present"], true);
        assert_eq!(result["valid_ref"]["media_bytes_received"], true);
        assert_eq!(result["valid_ref"]["failed"], false);
        for case in [
            "missing_key",
            "missing_path",
            "insufficient_path",
            "malformed_path",
            "encoded_group_is_literal",
            "empty_file_path",
            "denied_private_access",
            "absent_blob",
            "invalid_ref",
        ] {
            assert_eq!(result[case]["response_received"], true, "case {case}: {result}");
            assert_eq!(result[case]["failed"], false, "case {case}: {result}");
            assert_eq!(result[case]["response_is_false"], true, "case {case}: {result}");
            assert_eq!(result[case]["metadata_present"], false, "case {case}: {result}");
            assert_eq!(result[case]["media_bytes_received"], false, "case {case}: {result}");
            assert_eq!(result[case]["private_canary_leaked"], false, "case {case}: {result}");
        }
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

fn seed_private_media_repository(
    root: &std::path::Path,
    source: &std::path::Path,
) -> io::Result<()> {
    fs::create_dir_all(source)?;
    run_git(source, &["init", "-q"])?;
    run_git(source, &["config", "user.email", "private-media@example.invalid"])?;
    run_git(source, &["config", "user.name", "private-media-fixture"])?;
    fs::write(source.join("secret.bin"), b"PRIVATE_MEDIA_CANARY_613")?;
    run_git(source, &["add", "secret.bin"])?;
    run_git(source, &["commit", "-qm", "private media fixture"])?;
    let private_repo = root.join("private/repo");
    let private_repo_url = private_repo.to_string_lossy().into_owned();
    run_git(source, &["remote", "add", "origin", &private_repo_url])?;
    run_git(source, &["push", "-q", "origin", "HEAD:main"])
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
    raise RuntimeError("invalid-ref Link did not establish")

def request(data, label):
    completed = threading.Event()
    result = {"case": label, "response_received": False, "failed": False}
    def response(receipt):
        value = receipt.response
        result["response_received"] = True
        result["metadata_present"] = receipt.metadata is not None
        if hasattr(value, "read"):
            payload = value.read()
            result["payload_hex"] = payload.hex()
            result["media_bytes_received"] = bool(payload)
            result["name"] = (receipt.metadata or {}).get("name", b"").decode("utf-8")
            result["private_canary_leaked"] = b"PRIVATE_MEDIA_CANARY_613" in payload
        elif isinstance(value, (bytes, bytearray)):
            result["media_bytes_received"] = bool(value)
            result["private_canary_leaked"] = b"PRIVATE_MEDIA_CANARY_613" in value
        else:
            result["response_is_false"] = value is False
            result["media_bytes_received"] = False
            result["private_canary_leaked"] = False
        completed.set()
    def failed(receipt):
        result["failed"] = True
        completed.set()
    receipt = link.request("/media", data, response_callback=response, failed_callback=failed, timeout=4)
    if receipt is False:
        result["failed"] = True
    else:
        if not completed.wait(7):
            result["timed_out"] = True
    return result

key = b"issue-613-media-validation"
valid_path = "/media/group/repo/main/assets%2Fspace+name.bin"
valid = request({"key": key, "path": valid_path}, "valid_ref")
if not valid["response_received"] or valid.get("failed"):
    raise RuntimeError("known readable main-ref media request did not return a Resource")
denials = {
    "missing_key": request({"path": valid_path}, "missing_key"),
    "missing_path": request({"key": key}, "missing_path"),
    "insufficient_path": request({"key": key, "path": "/media/group/repo"}, "insufficient_path"),
    "malformed_path": request({"key": key, "path": "/not-media/group/repo/main/file.bin"}, "malformed_path"),
    "encoded_group_is_literal": request({"key": key, "path": "/media/%67roup/repo/main/assets%2Fspace+name.bin"}, "encoded_group_is_literal"),
    "empty_file_path": request({"key": key, "path": "/media/group/repo/main/"}, "empty_file_path"),
    "denied_private_access": request({"key": key, "path": "/media/private/repo/main/secret.bin"}, "denied_private_access"),
    "absent_blob": request({"key": key, "path": "/media/group/repo/main/assets%2Fabsent+file.bin"}, "absent_blob"),
    "invalid_ref": request({"key": key, "path": "/media/group/repo/no-such-ref-613/assets%2Fspace+name.bin"}, "invalid_ref"),
}
link.teardown()
print(json.dumps({"valid_ref": valid, **denials}, sort_keys=True))
"#;
