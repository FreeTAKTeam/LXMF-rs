#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_serves_pages_and_media_to_pinned_python_client() -> io::Result<()> {
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
        reference_pages.contains("urllib.parse.quote_plus(file_path)"),
        "pinned pages.py image markup must use quote_plus on the repository path"
    );

    let port = free_port()?;
    let identity_seed = "rngit-python-interop-server";
    if !Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success()
    {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "pinned Python rngit interop requires ffmpeg for the conversion trace",
        ));
    }
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
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, identity_seed)?;
        let config_dir = temp.path().join("python-client");
        fs::create_dir_all(&config_dir)?;
        write_python_config(&config_dir, port)?;
        let identity = config_dir.join("identity");
        let media_temp_directory = std::env::temp_dir();
        let output = run_python_client(
            &python_repo,
            &config_dir,
            &identity,
            &destination,
            &media_temp_directory,
        )?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python rngit client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("\"page_has_repository\": true"), "page response: {stdout}");
        let invalid_reference_body = format!(
            "#!c=0\n> Anonymous Git Node\n\n>>\n[Node`:/page/index.mu] / [group`:/page/group.mu|g=group] / repo `:/page/repo.mu|g=group|r=repo]\n\n>Not Found\n\nThe requested reference was not found\n\n<\n-\n`a`F666`[Served by rngit {}`:/page/index.mu] - local`f",
            env!("CARGO_PKG_VERSION")
        );
        assert!(
            stdout.contains(&serde_json::to_string(&invalid_reference_body)?),
            "invalid reference page rendering differs from its Link fixture: {stdout}"
        );
        assert!(stdout.contains("\"name\": \"image.png\""), "media metadata: {stdout}");
        assert!(
            stdout.contains(
                "\"sha256\": \"f8e920545e99cdc9bbc2650eb8282344e8971a7ff0c397c91355d0fcaf6c61fa\""
            ),
            "media checksum: {stdout}"
        );
        assert!(stdout.contains("\"size\": 8192"), "media size: {stdout}");
        assert!(stdout.contains("\"name\": \"valid.webp\""), "converted media metadata: {stdout}");
        assert!(stdout.contains("\"is_webp\": true"), "converted media payload: {stdout}");

        let git_destination = rust_git_destination(&root, identity_seed)?;
        let git_config_dir = temp.path().join("python-git-client");
        fs::create_dir_all(&git_config_dir)?;
        write_python_config(&git_config_dir, port)?;
        let git_identity = git_config_dir.join("identity");
        let git_output = run_python_git_client(
            &python_repo,
            &git_config_dir,
            &git_identity,
            &git_destination,
            &temp.path().join("source"),
        )?;
        if !git_output.status.success() {
            return Err(io::Error::other(format!(
                "Python rngit Git client failed: {}\nstdout:\n{}\nstderr:\n{}",
                git_output.status,
                String::from_utf8_lossy(&git_output.stdout),
                String::from_utf8_lossy(&git_output.stderr)
            )));
        }
        let git_stdout = String::from_utf8_lossy(&git_output.stdout);
        assert!(
            !root.join("private/repo.work").exists(),
            "denied work request created persistent work state"
        );
        assert!(git_stdout.contains("\"status\": 0"), "Git list status: {git_stdout}");
        assert!(git_stdout.contains("\"contains_main\": true"), "Git list payload: {git_stdout}");
        assert!(git_stdout.contains("\"fetch_status\": 0"), "Git fetch status: {git_stdout}");
        assert!(git_stdout.contains("\"fetch_valid\": true"), "Git fetch bundle: {git_stdout}");
        assert!(!git_stdout.contains("\"fetch_size\": 0"), "Git fetch was empty: {git_stdout}");
        assert!(git_stdout.contains("\"push_status\": 0"), "Git push status: {git_stdout}");
        assert!(
            git_stdout.contains("\"push_contains_python_ref\": true"),
            "Git push ref listing: {git_stdout}"
        );
        assert!(git_stdout.contains("\"delete_status\": 0"), "Git delete status: {git_stdout}");
        assert!(
            git_stdout.contains("\"delete_removed_python_ref\": true"),
            "Git delete ref listing: {git_stdout}"
        );
        assert!(git_stdout.contains("\"sync_status\": 0"), "Git sync status: {git_stdout}");
        assert!(
            git_stdout.contains("\"sync_contains_upstream_ref\": true"),
            "Git sync ref listing: {git_stdout}"
        );
        assert!(git_stdout.contains("\"fork_status\": 0"), "Git fork status: {git_stdout}");
        assert!(
            git_stdout.contains("\"fork_contains_main\": true"),
            "Git fork ref listing: {git_stdout}"
        );
        assert!(git_stdout.contains("\"mirror_status\": 0"), "Git mirror status: {git_stdout}");
        assert!(
            git_stdout.contains("\"mirror_contains_main\": true"),
            "Git mirror ref listing: {git_stdout}"
        );
        assert!(git_stdout.contains("\"create_status\": 0"), "Git create status: {git_stdout}");
        assert!(
            git_stdout.contains("\"create_registered_repository\": true"),
            "Git create listing: {git_stdout}"
        );
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_returns_raw_media_when_webp_backend_is_unavailable() -> io::Result<()> {
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

    let port = free_port()?;
    let identity_seed = "rngit-python-unavailable-media-backend";
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
        .env("RNGIT_MEDIA_BACKEND", "codex-test-backend-that-does-not-exist")
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
            .arg(RAW_FALLBACK_PYTHON_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python unavailable-backend media client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            io::Error::other(format!(
                "Python unavailable-backend client returned invalid JSON: {error}\nstdout:\n{}",
                String::from_utf8_lossy(&output.stdout)
            ))
        })?;
        assert_eq!(response["name"], "image.png");
        assert_eq!(response["size"], 8192);
        assert_eq!(response["matches_expected_raw_bytes"], true);
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

#[cfg(unix)]
#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn rngit_does_not_fall_through_from_unavailable_forced_backend() -> io::Result<()> {
    use std::os::unix::fs::{symlink, PermissionsExt};

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

    let isolated_path = temp.path().join("isolated-path");
    fs::create_dir(&isolated_path)?;
    let git_executable = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("git"))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "git executable not found on PATH"))?;
    symlink(git_executable, isolated_path.join("git"))?;

    let invocation_marker = temp.path().join("ffmpeg-invoked");
    let ffmpeg_stub = isolated_path.join("ffmpeg");
    fs::write(
        &ffmpeg_stub,
        "#!/bin/sh\nprintf 'invoked\\n' >> \"$RNGIT_TEST_FFMPEG_MARKER\"\nexit 1\n",
    )?;
    fs::set_permissions(&ffmpeg_stub, fs::Permissions::from_mode(0o755))?;
    let path = std::env::join_paths([isolated_path.as_os_str()]).map_err(io::Error::other)?;

    let port = free_port()?;
    let identity_seed = "rngit-python-forced-unavailable-magick";
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
        .env("RNGIT_MEDIA_BACKEND", "magick")
        .env("RNGIT_TEST_FFMPEG_MARKER", &invocation_marker)
        .env("PATH", path)
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
            .arg(RAW_FALLBACK_PYTHON_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python forced-unavailable-backend media client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            io::Error::other(format!(
                "Python forced-unavailable-backend client returned invalid JSON: {error}\nstdout:\n{}",
                String::from_utf8_lossy(&output.stdout)
            ))
        })?;
        assert_eq!(response["name"], "image.png");
        assert_eq!(response["size"], 8192);
        assert_eq!(response["matches_expected_raw_bytes"], true);
        assert!(
            !invocation_marker.exists(),
            "recognized but unavailable forced magick backend must not fall through to available ffmpeg"
        );
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

const RAW_FALLBACK_PYTHON_CLIENT: &str = r#"
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
finished = threading.Event()
result = {}
def response(receipt):
    value = receipt.response
    payload = value.read() if hasattr(value, "read") else value
    expected = bytes((index * 29) & 0xff for index in range(8192))
    result["name"] = receipt.metadata.get("name", b"").decode("utf-8") if receipt.metadata else None
    result["size"] = len(payload)
    result["sha256"] = hashlib.sha256(payload).hexdigest()
    result["matches_expected_raw_bytes"] = payload == expected
    finished.set()
def failed(_receipt):
    result["failed"] = True
    finished.set()
receipt = link.request("/media", {"key": b"raw-fallback", "path": "/media/group/repo/HEAD/image.png"}, response_callback=response, failed_callback=failed, timeout=4)
if receipt is False or not finished.wait(8):
    raise RuntimeError("raw media Resource did not complete")
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;
