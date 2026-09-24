use super::*;
use sha2::Digest;

#[cfg(unix)]
#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn rngit_disconnect_cleanup_preserves_an_independent_active_media_response() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let source = temp.path().join("source");
    let payload = (0..32 * 1024).map(|index| (index as u8).wrapping_mul(17)).collect::<Vec<_>>();
    fs::write(source.join("active-response.bin"), &payload)?;
    run_git(&source, &["add", "active-response.bin"])?;
    run_git(&source, &["commit", "-qm", "add independent cleanup response fixture"])?;
    run_git(&source, &["push", "-q", "origin", "main"])?;

    let media_temp_directory = temp.path().join("media-temp");
    let converter_directory = temp.path().join("converter");
    fs::create_dir(&media_temp_directory)?;
    fs::create_dir(&converter_directory)?;
    let converter = converter_directory.join("ffmpeg");
    fs::write(
        &converter,
        b"#!/bin/sh\ncat >/dev/null\nprintf 'RIFF\\036\\000\\000\\000WEBPVP8X\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000'\n",
    )?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&converter, fs::Permissions::from_mode(0o755))?;

    let mut search_paths = vec![converter_directory];
    search_paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(search_paths).map_err(io::Error::other)?;
    let port = free_port()?;
    let identity_seed = "rngit-cleanup-link-isolation";
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
        .env("RNGIT_MEDIA_BACKEND", "ffmpeg")
        .env("TMPDIR", &media_temp_directory)
        .env("TEMP", &media_temp_directory)
        .env("PATH", path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, identity_seed)?;
        let config_dir = temp.path().join("python-client");
        fs::create_dir(&config_dir)?;
        write_python_config(&config_dir, port)?;
        let expected_sha256 = format!("{:x}", sha2::Sha256::digest(&payload));
        let output = Command::new(python_bin())
            .arg("-c")
            .arg(PYTHON_CLEANUP_LINK_ISOLATION_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .arg(&media_temp_directory)
            .arg(&expected_sha256)
            .env("PYTHONPATH", python_repo())
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python Link-isolation client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("\"cleanup_link_temp_dirs\": 0"), "cleanup result: {stdout}");
        assert!(stdout.contains("\"active_response_sha256\""), "active response missing: {stdout}");
        assert!(stdout.contains(&expected_sha256), "active response checksum mismatch: {stdout}");
        wait_for_empty_directory(&media_temp_directory)
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

#[cfg(unix)]
const PYTHON_CLEANUP_LINK_ISOLATION_CLIENT: &str = r#"
import hashlib
import json
import os
import sys
import threading
import time
import RNS

config_dir, destination_hex, media_temp_directory, expected_sha256 = sys.argv[1:5]
RNS.Reticulum(configdir=config_dir, loglevel=0)
destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
remote_identity = RNS.Identity.recall(destination_hash)
if remote_identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(remote_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node")

def connect_link():
    ready = threading.Event()
    link = RNS.Link(destination)
    link.set_link_established_callback(lambda _link: ready.set())
    if not ready.wait(30):
        raise RuntimeError("Link establishment timed out")
    return link

cleanup_link = connect_link()
converted = threading.Event()
converted_result = {}
def converted_response(receipt):
    value = receipt.response.read() if hasattr(receipt.response, "read") else receipt.response
    converted_result["name"] = receipt.metadata.get("name", b"").decode("utf-8") if receipt.metadata else None
    converted_result["valid_webp"] = value[:12] == b"RIFF\x1e\x00\x00\x00WEBP"
    converted.set()
if cleanup_link.request("/media", {"key": b"isolation", "path": "/media/group/repo/HEAD/valid.png"}, response_callback=converted_response, timeout=10) is False:
    raise RuntimeError("cleanup Link media request was not sent")
if not converted.wait(20) or converted_result != {"name": "valid.webp", "valid_webp": True}:
    raise RuntimeError(f"converted response failed: {converted_result}")
deadline = time.monotonic() + 5
while time.monotonic() < deadline and not os.listdir(media_temp_directory):
    time.sleep(0.01)
if len(os.listdir(media_temp_directory)) != 1:
    raise RuntimeError(f"expected one tracked conversion directory: {os.listdir(media_temp_directory)}")

active_link = connect_link()
progress_entered = threading.Event()
release_progress = threading.Event()
progress_returned = threading.Event()
active_response = threading.Event()
active_result = {}
def progress(receipt):
    if 0.0 < receipt.progress < 1.0 and not progress_entered.is_set():
        progress_entered.set()
        if not release_progress.wait(10):
            raise RuntimeError("test did not release active response progress callback")
    progress_returned.set()
def response(receipt):
    value = receipt.response.read() if hasattr(receipt.response, "read") else receipt.response
    active_result["sha256"] = hashlib.sha256(value).hexdigest()
    active_result["size"] = len(value)
    active_result["completed_at"] = time.monotonic()
    active_response.set()
def failed(receipt):
    active_result["failed"] = str(receipt.status)
    active_response.set()
if active_link.request("/media", {"key": b"active", "path": "/media/group/repo/HEAD/active-response.bin"}, response_callback=response, failed_callback=failed, progress_callback=progress, timeout=60) is False:
    raise RuntimeError("active response request was not sent")
if not progress_entered.wait(30):
    raise RuntimeError("active response never entered a partial-Resource callback")
cleanup_link.teardown()
deadline = time.monotonic() + 5
while time.monotonic() < deadline and os.listdir(media_temp_directory):
    time.sleep(0.01)
if os.listdir(media_temp_directory):
    release_progress.set()
    raise RuntimeError("disconnect did not clean the converted-media directory during the independent response")
cleanup_completed_at = time.monotonic()
release_progress.set()
if not progress_returned.wait(5):
    raise RuntimeError("active response progress callback did not resume")

if not active_response.wait(30):
    raise RuntimeError(f"active response did not complete; status={active_link.status}; result={active_result}")
if active_result.get("sha256") != expected_sha256 or active_result.get("size") != 32 * 1024:
    raise RuntimeError(f"active response was corrupted: {active_result}")
if active_result["completed_at"] < cleanup_completed_at:
    raise RuntimeError("independent media response completed before the other Link cleanup")
active_link.teardown()
deadline = time.monotonic() + 5
while time.monotonic() < deadline and os.listdir(media_temp_directory):
    time.sleep(0.01)
remaining = os.listdir(media_temp_directory)
if remaining:
    raise RuntimeError(f"disconnect did not clean only its temporary data: {remaining}")
print(json.dumps({"cleanup_link_temp_dirs": len(remaining), "active_response_sha256": active_result["sha256"], "active_response_size": active_result["size"]}, sort_keys=True))
"#;
