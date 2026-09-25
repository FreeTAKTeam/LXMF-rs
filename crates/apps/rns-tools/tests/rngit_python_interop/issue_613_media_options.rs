use super::*;
#[cfg(unix)]
use std::io::Read;

#[cfg(unix)]
#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn rngit_passes_media_cli_options_to_the_selected_webp_backend() -> io::Result<()> {
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

    let backend_dir = temp.path().join("backend");
    fs::create_dir(&backend_dir)?;
    let backend = backend_dir.join("ffmpeg");
    fs::write(
        &backend,
        b"#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$RNGIT_TEST_MEDIA_ARGS\"\ncat >/dev/null\nprintf 'RIFF\\036\\000\\000\\000WEBPVP8X\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000'\n",
    )?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&backend, fs::Permissions::from_mode(0o755))?;

    let args_path = temp.path().join("ffmpeg-args");
    let mut search_paths = vec![backend_dir];
    search_paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(search_paths).map_err(io::Error::other)?;
    let port = free_port()?;
    let identity_seed = "rngit-cli-media-options";
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            identity_seed,
            "--media-quality",
            "37",
            "--media-max-dimension",
            "321",
            "--silent",
        ])
        .env("RNGIT_MEDIA_BACKEND", "ffmpeg")
        .env("RNGIT_TEST_MEDIA_ARGS", &args_path)
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
            .arg(PYTHON_MEDIA_OPTIONS_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python media-options client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                io::Error::other(format!(
                    "Python media-options result was invalid JSON: {error}\nstdout:\n{}",
                    String::from_utf8_lossy(&output.stdout)
                ))
            })?;
        assert_eq!(response["name"], "image.webp");
        assert_eq!(response["webp"], true);

        let args = fs::read_to_string(&args_path)?;
        let args = args.lines().collect::<Vec<_>>();
        assert!(args.windows(2).any(|pair| pair == ["-quality", "37"]), "argv: {args:?}");
        assert!(
            args.windows(2).any(|pair| {
                pair == [
                    "-vf",
                    "scale='min(iw,321)':'min(ih,321)':force_original_aspect_ratio=decrease",
                ]
            }),
            "argv: {args:?}"
        );
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

#[cfg(unix)]
#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn rngit_invokes_selected_avconv_backend_with_argument_safe_options() -> io::Result<()> {
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

    let backend_dir = temp.path().join("avconv-backend");
    fs::create_dir(&backend_dir)?;
    let backend = backend_dir.join("avconv");
    fs::write(
        &backend,
        b"#!/bin/sh\nprintf '%s\\0' \"$@\" > \"$RNGIT_TEST_MEDIA_ARGS\"\ncat >/dev/null\nprintf 'RIFF\\036\\000\\000\\000WEBPVP8X\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000'\n",
    )?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&backend, fs::Permissions::from_mode(0o755))?;

    let args_path = temp.path().join("avconv-args");
    let mut search_paths = vec![backend_dir];
    search_paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(search_paths).map_err(io::Error::other)?;
    let port = free_port()?;
    let identity_seed = "rngit-avconv-argv-link-regression";
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            identity_seed,
            "--media-quality",
            "37",
            "--media-max-dimension",
            "321",
            "--silent",
        ])
        .env("RNGIT_MEDIA_BACKEND", "avconv")
        .env("RNGIT_TEST_MEDIA_ARGS", &args_path)
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
            .arg(PYTHON_MEDIA_OPTIONS_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python avconv-options client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                io::Error::other(format!(
                    "Python avconv-options result was invalid JSON: {error}\nstdout:\n{}",
                    String::from_utf8_lossy(&output.stdout)
                ))
            })?;
        assert_eq!(response["name"], "image.webp");
        assert_eq!(response["webp"], true);

        let args = fs::read(&args_path)?;
        let args = args
            .split(|byte| *byte == 0)
            .filter(|arg| !arg.is_empty())
            .map(|arg| String::from_utf8_lossy(arg).into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "-y",
                "-loglevel",
                "error",
                "-i",
                "-",
                "-quality",
                "37",
                "-vf",
                "scale='min(iw,321)':'min(ih,321)':force_original_aspect_ratio=decrease",
                "-f",
                "webp",
                "pipe:1",
            ]
        );
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

#[cfg(unix)]
#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn rngit_failed_webp_encoder_returns_raw_media_and_cleans_conversion_directory() -> io::Result<()> {
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

    let backend_dir = temp.path().join("failing-backend");
    let media_temp = temp.path().join("media-temp");
    fs::create_dir(&backend_dir)?;
    fs::create_dir(&media_temp)?;
    let backend = backend_dir.join("ffmpeg");
    fs::write(
        &backend,
        b"#!/bin/sh\ncat >/dev/null\nprintf 'injected encoder failure' >&2\nexit 23\n",
    )?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&backend, fs::Permissions::from_mode(0o755))?;

    let mut search_paths = vec![backend_dir];
    search_paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(search_paths).map_err(io::Error::other)?;
    let port = free_port()?;
    let identity_seed = "rngit-cli-media-conversion-failure";
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
        .env("TMPDIR", &media_temp)
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
            .arg(PYTHON_MEDIA_FAILURE_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python media-failure client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                io::Error::other(format!(
                    "Python media-failure result was invalid JSON: {error}\nstdout:\n{}",
                    String::from_utf8_lossy(&output.stdout)
                ))
            })?;
        assert_eq!(response["name"], "valid.png");
        assert_eq!(response["size"], 68);
        assert_eq!(
            response["sha256"],
            "431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460"
        );
        wait_for_empty_directory(&media_temp)?;

        Ok(())
    })();
    if server.try_wait()?.is_none() {
        let _ = server.kill();
    }
    let _ = server.wait();
    let mut diagnostics = String::new();
    if let Some(mut stderr) = server.stderr.take() {
        stderr.read_to_string(&mut diagnostics)?;
    }
    result?;
    assert!(
        diagnostics.contains("injected encoder failure"),
        "backend failure detail should be visible in server diagnostics: {diagnostics}"
    );
    Ok(())
}

const PYTHON_MEDIA_OPTIONS_CLIENT: &str = r#"
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
    result["name"] = receipt.metadata.get("name", b"").decode("utf-8") if receipt.metadata else None
    result["webp"] = payload[:12] == b"RIFF\x1e\x00\x00\x00WEBP"
    finished.set()
def failed(_receipt):
    result["failed"] = True
    finished.set()
receipt = link.request("/media", {"key": b"configured-media", "path": "/media/group/repo/HEAD/image.png"}, response_callback=response, failed_callback=failed, timeout=4)
if receipt is False or not finished.wait(8):
    raise RuntimeError("configured WebP Resource did not complete")
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;

#[cfg(unix)]
const PYTHON_MEDIA_FAILURE_CLIENT: &str = r#"
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
    payload = receipt.response.read()
    result["name"] = receipt.metadata.get("name", b"").decode("utf-8") if receipt.metadata else None
    result["size"] = len(payload)
    result["sha256"] = hashlib.sha256(payload).hexdigest()
    finished.set()
def failed(_receipt):
    result["failed"] = True
    finished.set()
receipt = link.request("/media", {"key": b"encoder-failure", "path": "/media/group/repo/HEAD/valid.png"}, response_callback=response, failed_callback=failed, timeout=4)
if receipt is False or not finished.wait(8):
    raise RuntimeError("raw fallback Resource did not complete")
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;
