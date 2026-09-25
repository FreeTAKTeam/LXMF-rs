use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

const PYTHON_RETICULUM_PARITY_REF: &str = "99de23c040d507e3fefca19e87b182302902725d";

#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn rngit_media_source_stream_open_failure_has_no_response_and_keeps_link_live() -> io::Result<()> {
    let _test_guard =
        super::PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let python_repo = python_repo();
    let reference_revision = Command::new("git")
        .args(["-C", python_repo.to_string_lossy().as_ref(), "rev-parse", "HEAD"])
        .output()?;
    if !reference_revision.status.success()
        || String::from_utf8_lossy(&reference_revision.stdout).trim() != PYTHON_RETICULUM_PARITY_REF
    {
        return Err(io::Error::other(format!(
            "test requires pinned Reticulum {PYTHON_RETICULUM_PARITY_REF}, found {}",
            String::from_utf8_lossy(&reference_revision.stdout).trim()
        )));
    }
    let reference_pages = fs::read_to_string(python_repo.join("RNS/Utilities/rngit/pages.py"))?;
    assert!(
        reference_pages.contains("result = subprocess.run([\"git\", \"cat-file\", \"-s\", f\"{ref}:{file_path}\"]")
            && reference_pages.contains("result = subprocess.run([\"git\", \"ls-tree\", ls_tree_path]")
            && reference_pages.contains("proc = subprocess.Popen([\"git\", \"show\", f\"{ref}:{file_path}\"]")
            && reference_pages.contains("except Exception as e:            RNS.log(f\"Error getting blob content handle: {e}\", RNS.LOG_WARNING)")
            && reference_pages.contains("if stream: return [stream, {\"name\": response_name.encode(\"utf-8\")}]\n            else:")
            && reference_pages.contains("Could not resolve blob stream for media request"),
        "pinned pages.py must map source-stream process-open failure to no response"
    );
    let reference_resource = fs::read_to_string(python_repo.join("RNS/Resource.py"))?;
    assert!(
        reference_resource.contains("if hasattr(data, \"read\"):")
            && reference_resource.contains("data_size = os.stat(data.name).st_size"),
        "pinned Resource must consume a source stream only after pages.py returns one"
    );

    let repository = root.join("group/repo");
    let moved_repository = temp.path().join("repo-moved-after-blob-info");
    let fault_marker = temp.path().join("source-open-failed");
    let real_git = Command::new("which").arg("git").output()?;
    if !real_git.status.success() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "git executable not found"));
    }
    let real_git = String::from_utf8_lossy(&real_git.stdout).trim().to_string();
    let wrapper_dir = temp.path().join("git-wrapper");
    fs::create_dir(&wrapper_dir)?;
    let wrapper = wrapper_dir.join("git");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"cat-file\" ] && [ \"$2\" = \"-s\" ] && [ ! -e \"$RNGIT_SOURCE_OPEN_MARKER\" ]; then\n  '{real_git}' \"$@\"\n  status=$?\n  if [ \"$status\" -eq 0 ]; then\n    ref=\"${{3%%:*}}\"\n    '{real_git}' ls-tree \"$ref\" >/dev/null || exit 92\n    mv \"$RNGIT_SOURCE_REPO\" \"$RNGIT_MOVED_REPO\" || exit 91\n    : > \"$RNGIT_SOURCE_OPEN_MARKER\"\n  fi\n  exit \"$status\"\nfi\nexec '{real_git}' \"$@\"\n"
        ),
    )?;
    let mut permissions = fs::metadata(&wrapper)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&wrapper, permissions)?;

    let media_temp = temp.path().join("media-temp");
    fs::create_dir(&media_temp)?;
    let port = free_port()?;
    let identity_seed = "rngit-python-media-source-open-failure";
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
        .env("PATH", format!("{}:/usr/bin:/bin", wrapper_dir.display()))
        .env("TMPDIR", &media_temp)
        .env("RNGIT_SOURCE_REPO", &repository)
        .env("RNGIT_MOVED_REPO", &moved_repository)
        .env("RNGIT_SOURCE_OPEN_MARKER", &fault_marker)
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
            .arg(&moved_repository)
            .arg(&repository)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python media source-open-failure client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let observed: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            io::Error::other(format!(
                "Python media source-open-failure client returned invalid JSON: {error}\nstdout:\n{}",
                String::from_utf8_lossy(&output.stdout)
            ))
        })?;
        assert_eq!(observed["first_response"], false, "response: {observed}");
        assert_eq!(observed["first_failure"], false, "response: {observed}");
        assert_eq!(observed["recovery_name"], "README.md", "response: {observed}");
        assert_eq!(
            observed["recovery_payload"], "# Python rngit interop\n",
            "response: {observed}"
        );
        assert_eq!(observed["recovery_size"], 23, "response: {observed}");
        assert_eq!(
            observed["recovery_sha256"],
            "100574e8552cbce2f3023f1ee66544c11c87208d5d9db5dc5eb8388cca586d2b",
            "response: {observed}"
        );
        assert!(fault_marker.is_file(), "the source-open race was not injected");
        assert!(repository.is_dir(), "the repository path was not restored");
        assert!(
            fs::read_dir(&media_temp)?.next().is_none(),
            "non-image source-open failure must not leave conversion artifacts"
        );
        assert!(server.try_wait()?.is_none(), "source-open failure terminated rngit");
        Ok(())
    })();

    if moved_repository.exists() && !repository.exists() {
        fs::rename(&moved_repository, &repository)?;
    }
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
import time
import RNS

config_dir, destination_hex, moved_repository, repository = sys.argv[1:5]
RNS.Reticulum(configdir=config_dir, loglevel=0)
destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
identity = RNS.Identity.recall(destination_hash)
if identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node")
ready = threading.Event()
link = RNS.Link(destination)
link.set_link_established_callback(lambda _: ready.set())
link.set_link_closed_callback(lambda _: ready.set())
if not ready.wait(30) or link.status != RNS.Link.ACTIVE:
    raise RuntimeError("media source-open-failure Link did not establish")

first_response = threading.Event()
first_failure = threading.Event()
second_response = threading.Event()
result = {}
def first_success(_): first_response.set()
def first_failed(_): first_failure.set()
def second_success(receipt):
    payload = receipt.response.read()
    metadata = receipt.metadata or {}
    name = metadata.get("name", b"")
    result.update({
        "recovery_name": name.decode("utf-8") if isinstance(name, bytes) else str(name),
        "recovery_payload": payload.decode("utf-8"),
        "recovery_size": len(payload),
        "recovery_sha256": hashlib.sha256(payload).hexdigest(),
    })
    second_response.set()
def second_failed(receipt):
    result["recovery_failed"] = str(receipt.status)
    second_response.set()

link.request("/media", {"key": b"present", "path": "/media/group/repo/HEAD/README.md"},
             response_callback=first_success, failed_callback=first_failed, timeout=30)
deadline = time.monotonic() + 10
while not os.path.isdir(moved_repository) and time.monotonic() < deadline:
    if first_response.is_set() or first_failure.is_set():
        raise RuntimeError("source-stream creation failure unexpectedly generated a response")
    time.sleep(0.02)
if not os.path.isdir(moved_repository):
    raise RuntimeError("source-open fault injector did not move the source repository cwd")
if first_response.is_set() or first_failure.is_set():
    raise RuntimeError("source-stream creation failure unexpectedly generated a response")
os.rename(moved_repository, repository)
link.request("/media", {"key": b"present", "path": "/media/group/repo/HEAD/README.md"},
             response_callback=second_success, failed_callback=second_failed, timeout=30)
if not second_response.wait(30):
    raise RuntimeError("valid media request did not recover after restoring the source")
if first_response.is_set() or first_failure.is_set():
    raise RuntimeError("first no-response request unexpectedly completed during recovery")
link.teardown()
print(json.dumps({
    "first_response": first_response.is_set(),
    "first_failure": first_failure.is_set(),
    **result,
}, sort_keys=True))
"#;
