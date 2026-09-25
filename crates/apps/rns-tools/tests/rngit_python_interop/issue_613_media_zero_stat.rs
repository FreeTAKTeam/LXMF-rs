use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::process::{Command, Stdio};

const PYTHON_RETICULUM_PARITY_REF: &str = "99de23c040d507e3fefca19e87b182302902725d";

#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn rngit_media_zero_stat_and_tree_paths_match_pinned_show_behavior() -> io::Result<()> {
    let _test_guard =
        super::PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let python_repo = python_repo();
    let revision = Command::new("git")
        .args(["-C", python_repo.to_string_lossy().as_ref(), "rev-parse", "HEAD"])
        .output()?;
    if !revision.status.success()
        || String::from_utf8_lossy(&revision.stdout).trim() != PYTHON_RETICULUM_PARITY_REF
    {
        return Err(io::Error::other(format!(
            "test requires pinned Reticulum {PYTHON_RETICULUM_PARITY_REF}, found {}",
            String::from_utf8_lossy(&revision.stdout).trim()
        )));
    }

    // The Python /media path returns a git-show stdout pipe. fstat on that
    // pipe reports zero regardless of the stream length; Resource.py's zero
    // size branch proxies the stream before constructing the Resource.
    let pages = fs::read_to_string(python_repo.join("RNS/Utilities/rngit/pages.py"))?;
    assert!(pages.contains("subprocess.Popen([\"git\", \"show\", f\"{ref}:{file_path}\"]"));
    assert!(pages.contains("return [stream, {\"name\": response_name.encode(\"utf-8\")}]"));
    assert!(pages.contains(
        "result = subprocess.run([\"git\", \"cat-file\", \"-s\", f\"{ref}:{file_path}\"]"
    ));
    assert!(pages.contains("def get_blob_stream(self, repo_path, ref, path):"));
    let resource = fs::read_to_string(python_repo.join("RNS/Resource.py"))?;
    assert!(resource.contains("data_size = os.stat(data.name).st_size"));
    assert!(resource.contains("if data_size == 0:"));
    assert!(resource.contains("stream_proxy.write(data.read())"));

    let source = temp.path().join("source");
    fs::write(source.join("empty.bin"), [])?;
    fs::write(
        source.join("nonempty.bin"),
        b"deterministic non-empty rngit media payload\nsecond line\n",
    )?;
    run_git(&source, &["add", "empty.bin", "nonempty.bin"])?;
    run_git(&source, &["commit", "-qm", "add media blobs"])?;
    run_git(&source, &["push", "-q", "origin", "main"])?;

    let port = free_port()?;
    let identity_seed = "rngit-python-media-zero-stat";
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
                "Python media zero-stat client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let observed: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| io::Error::other(format!("invalid Python client JSON: {error}")))?;
        assert_eq!(observed["response_received"], true, "{observed}");
        assert_eq!(observed["failed"], false, "{observed}");
        assert_eq!(observed["name"], "empty.bin", "{observed}");
        assert_eq!(observed["payload_size"], 0, "{observed}");
        assert_eq!(observed["nonempty_name"], "nonempty.bin", "{observed}");
        assert_eq!(
            observed["nonempty_payload_hex"],
            "64657465726d696e6973746963206e6f6e2d656d70747920726e676974206d65646961207061796c6f61640a7365636f6e64206c696e650a",
            "{observed}"
        );
        assert_eq!(
            observed["nonempty_sha256"],
            "c4fdc750e63ae70698abc583334e06565721fa98be73cabbe52084198a7159e1",
            "{observed}"
        );
        assert_eq!(observed["nonempty_payload_size"], 56, "{observed}");
        let resolved =
            Command::new("git").args(["rev-parse", "main"]).current_dir(&source).output()?;
        if !resolved.status.success() {
            return Err(io::Error::other("could not resolve the media fixture commit"));
        }
        let tree_spec = format!("{}:assets", String::from_utf8_lossy(&resolved.stdout).trim());
        let tree = Command::new("git").args(["show", &tree_spec]).current_dir(&source).output()?;
        if !tree.status.success() {
            return Err(io::Error::other("reference Git tree fixture could not be read"));
        }
        assert_eq!(observed["tree_received"], true, "{observed}");
        assert_eq!(observed["tree_name"], "assets", "{observed}");
        assert_eq!(observed["tree_payload_hex"], hex_bytes(&tree.stdout), "{observed}");
        assert_eq!(observed["link_active"], true, "{observed}");
        assert!(server.try_wait()?.is_none(), "zero-size stat case terminated rngit");
        Ok(())
    })();

    let _ = server.kill();
    let _ = server.wait();
    result
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[(byte >> 4) as usize]));
        encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    encoded
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
identity = RNS.Identity.recall(destination_hash)
if identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node")
ready = threading.Event()
link = RNS.Link(destination)
link.set_link_established_callback(lambda _: ready.set())
link.set_link_closed_callback(lambda _: ready.set())
if not ready.wait(30) or link.status != RNS.Link.ACTIVE:
    raise RuntimeError("media zero-stat Link did not establish")

finished = threading.Event()
def request_media(path):
    finished = threading.Event()
    result = {"response_received": False, "failed": False}
    def response(receipt):
        payload = receipt.response.read() if hasattr(receipt.response, "read") else receipt.response
        metadata = receipt.metadata or {}
        name = metadata.get("name", b"")
        result.update({
            "response_received": True,
            "name": name.decode("utf-8") if isinstance(name, bytes) else str(name),
            "payload_size": len(payload) if isinstance(payload, bytes) else -1,
            "payload_hex": payload.hex() if isinstance(payload, bytes) else "",
            "sha256": __import__("hashlib").sha256(payload).hexdigest() if isinstance(payload, bytes) else "",
        })
        finished.set()
    def failed(receipt):
        result["failed"] = True
        result["failure_status"] = str(receipt.status)
        finished.set()
    link.request("/media", {"key": b"present", "path": path},
                 response_callback=response, failed_callback=failed, timeout=20)
    if not finished.wait(25):
        raise RuntimeError("media Resource response did not complete: " + path)
    return result

empty = request_media("/media/group/repo/HEAD/empty.bin")
nonempty = request_media("/media/group/repo/HEAD/nonempty.bin")
tree = request_media("/media/group/repo/HEAD/assets")
if not empty["response_received"] or empty["failed"]:
    raise RuntimeError("empty-blob media response failed: " + repr(empty))
if not nonempty["response_received"] or nonempty["failed"]:
    raise RuntimeError("nonempty-blob media response failed: " + repr(nonempty))
if not tree["response_received"] or tree["failed"]:
    raise RuntimeError("tree media response failed: " + repr(tree))
result = {
    "response_received": empty["response_received"],
    "failed": empty["failed"],
    "name": empty["name"],
    "payload_size": empty["payload_size"],
    "nonempty_name": nonempty["name"],
    "nonempty_payload_hex": nonempty["payload_hex"],
    "nonempty_sha256": nonempty["sha256"],
    "nonempty_payload_size": nonempty["payload_size"],
    "tree_received": tree["response_received"],
    "tree_name": tree["name"],
    "tree_payload_hex": tree["payload_hex"],
}
result["link_active"] = link.status == RNS.Link.ACTIVE
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;
