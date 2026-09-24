use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config, PYTHON_INTEROP_TEST_LOCK,
};
use std::fs;
use std::io;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn pinned_python_media_backend_selection_reuses_available_automatic_winner() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let helper = python_repo().join("RNS/Utilities/rngit/media.py");
    if !helper.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rngit media helper not found: {}", helper.display()),
        ));
    }
    let script = r#"
import importlib.util
import sys
import types

sys.modules["RNS"] = types.SimpleNamespace(log=lambda *_args: None, LOG_WARNING=2)
spec = importlib.util.spec_from_file_location("rngit_media_reference", sys.argv[1])
media = importlib.util.module_from_spec(spec)
spec.loader.exec_module(media)
available = {"ffmpeg"}
media.shutil.which = lambda program: program if program in available else None
assert media._selected_backend()[0] == "ffmpeg"
available.add("magick")
assert media._selected_backend()[0] == "ffmpeg"
available.remove("ffmpeg")
assert media._selected_backend()[0] == "magick"
"#;
    let output = Command::new(python_bin())
        .arg("-c")
        .arg(script)
        .arg(helper)
        .env_remove("RNGIT_MEDIA_BACKEND")
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "pinned Python backend-selection regression failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_media_resource_preserves_precompressed_png_without_resource_compression() -> io::Result<()>
{
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let python_repo = python_repo();
    if !python_repo.join("RNS/Link.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }
    let reference_pages = fs::read_to_string(python_repo.join("RNS/Utilities/rngit/pages.py"))?;
    assert!(
        reference_pages.contains(
            "self.destination.register_request_handler(self.PATH_MEDIA,    response_generator=self.serve_media,         allow=RNS.Destination.ALLOW_ALL, auto_compress=False)"
        ),
        "pinned pages.py must register PATH_MEDIA with auto_compress=False"
    );

    let port = free_port()?;
    let identity_seed = "rngit-python-precompressed-media";
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            identity_seed,
            "--no-media-conversion",
            "--silent",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
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
                "Python precompressed-media client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                io::Error::other(format!(
                    "Python precompressed-media result was invalid JSON: {error}\nstdout:\n{}",
                    String::from_utf8_lossy(&output.stdout)
                ))
            })?;
        assert_eq!(response["advertisement_compressed"], false);
        assert_eq!(response["name"], "valid.png");
        assert_eq!(response["size"], 68);
        assert_eq!(response["matches_fixture_bytes"], true);
        assert_eq!(
            response["sha256"],
            "431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460"
        );
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
remote_identity = RNS.Identity.recall(destination_hash)
if remote_identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(remote_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node")
ready = threading.Event()
link = RNS.Link(destination)
link.set_link_established_callback(lambda _link: ready.set())
if not ready.wait(30):
    raise RuntimeError("media Link establishment timed out")

advertisements = []
original_accept = RNS.Resource.accept
def observe_accept(packet, *args, **kwargs):
    if RNS.ResourceAdvertisement.is_response(packet):
        advertisements.append(RNS.ResourceAdvertisement.unpack(packet.plaintext).is_compressed())
    return original_accept(packet, *args, **kwargs)
RNS.Resource.accept = staticmethod(observe_accept)
finished = threading.Event()
result = {}
def response(receipt):
    payload = receipt.response.read()
    expected = bytes([
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x04, 0x00, 0x00, 0x00, 0xb5, 0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00,
        0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0xf8, 0x0f, 0x00,
        0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ])
    result["name"] = (receipt.metadata or {}).get("name", b"").decode("utf-8")
    result["size"] = len(payload)
    result["sha256"] = hashlib.sha256(payload).hexdigest()
    result["matches_fixture_bytes"] = payload == expected
    finished.set()
def failed(_receipt):
    result["failed"] = True
    finished.set()

receipt = link.request(
    "/media",
    {"key": b"precompressed-media", "path": "/media/group/repo/HEAD/valid.png"},
    response_callback=response,
    failed_callback=failed,
    timeout=30,
)
try:
    if receipt is False or not finished.wait(32):
        raise RuntimeError("precompressed media Resource did not complete")
    if result.get("failed"):
        raise RuntimeError("precompressed media Resource failed")
finally:
    RNS.Resource.accept = staticmethod(original_accept)
link.teardown()
if len(advertisements) != 1:
    raise RuntimeError("expected exactly one Resource response advertisement")
result["advertisement_compressed"] = advertisements[0]
print(json.dumps(result, sort_keys=True))
"#;
