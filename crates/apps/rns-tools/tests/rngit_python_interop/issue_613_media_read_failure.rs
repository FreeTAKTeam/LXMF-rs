use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config, PYTHON_INTEROP_TEST_LOCK,
};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn rngit_media_content_read_failure_has_no_python_link_response() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let python_repo = python_repo();
    if !python_repo.join("RNS/Utilities/rngit/pages.py").is_file()
        || !python_repo.join("RNS/Link.py").is_file()
    {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }

    // Let object-info queries succeed, then fail only the content-stream read.
    // This reproduces get_blob_info() succeeding before get_blob_stream() fails.
    let wrapper_dir = temp.path().join("git-wrapper");
    fs::create_dir(&wrapper_dir)?;
    let marker = temp.path().join("content-read-attempted");
    let real_git = Command::new("which").arg("git").output()?;
    if !real_git.status.success() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "git executable not found"));
    }
    let real_git = String::from_utf8_lossy(&real_git.stdout).trim().to_string();
    let wrapper = wrapper_dir.join("git");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\ncase \"$1:$2\" in\n  cat-file:blob) touch '{}' ; exit 1 ;;\nesac\nexec '{}' \"$@\"\n",
            marker.display(),
            real_git
        ),
    )?;
    let mut permissions = fs::metadata(&wrapper)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&wrapper, permissions)?;

    let port = free_port()?;
    let identity_seed = "rngit-python-media-read-failure";
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
                "Python media read-failure client failed: {}\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let observed: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| io::Error::other(format!("invalid client JSON: {error}")))?;
        assert_eq!(observed["callback_received"], false, "client result: {observed}");
        assert_eq!(observed["failed_callback"], false, "client result: {observed}");
        assert!(marker.is_file(), "fault was not injected after object-info resolution");
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

const PYTHON_CLIENT: &str = r#"
import json, sys, threading
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
    raise RuntimeError("media read-failure Link did not establish")
callback = threading.Event()
failed = threading.Event()
link.request("/media", {"key": b"present", "path": "/media/group/repo/main/image.png"},
             response_callback=lambda _: callback.set(),
             failed_callback=lambda _: failed.set(), timeout=2)
threading.Event().wait(4)
print(json.dumps({"callback_received": callback.is_set(), "failed_callback": failed.is_set()}))
link.teardown()
"#;
