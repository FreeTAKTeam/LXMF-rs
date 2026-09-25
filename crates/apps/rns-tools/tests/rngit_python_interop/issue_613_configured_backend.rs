use super::*;

#[cfg(unix)]
#[test]
#[ignore = "requires local pinned Python Reticulum checkout, selected WebP backend, and ffprobe"]
fn configured_backend_serves_decodable_webp_and_falls_back_to_raw_media() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let backend = std::env::var("RNGIT_TEST_WEBP_BACKEND").map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "RNGIT_TEST_WEBP_BACKEND must select a real encoder",
        )
    })?;
    if !["magick", "convert", "gm", "ffmpeg", "avconv"].contains(&backend.as_str()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported test backend name: {backend}"),
        ));
    }
    for tool in [backend.as_str(), "ffprobe"] {
        let status = Command::new(tool)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(io::Error::other(format!(
                "required WebP test tool {tool} version probe exited with {status}"
            )));
        }
    }
    let resolved_backend = Command::new("which").arg(&backend).output()?;
    if !resolved_backend.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("required native WebP backend is not on PATH: {backend}"),
        ));
    }
    let backend_executable = String::from_utf8_lossy(&resolved_backend.stdout).trim().to_owned();

    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let source = temp.path().join("source");
    let png = Command::new(python_bin())
        .arg("-c")
        .arg(PNG_FIXTURE_WRITER)
        .arg(source.join("valid.png"))
        .output()?;
    if !png.status.success() {
        return Err(io::Error::other(format!(
            "could not prepare multi-pixel PNG fixture: {}",
            String::from_utf8_lossy(&png.stderr)
        )));
    }
    run_git(&source, &["add", "valid.png"])?;
    run_git(&source, &["commit", "--amend", "--no-edit", "-q"])?;
    run_git(&source, &["push", "--force", "origin", "main"])?;
    let python_repo = python_repo();
    if !python_repo.join("RNS/Link.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }
    let media_temp = temp.path().join("media-temp");
    fs::create_dir(&media_temp)?;
    let backend_dir = temp.path().join("backend-probe");
    fs::create_dir(&backend_dir)?;
    let args_path = temp.path().join("backend-argv");
    let backend_probe = backend_dir.join(&backend);
    fs::write(
        &backend_probe,
        format!(
            "#!/bin/sh\nprintf '%s\\n' --BEGIN-- \"$@\" >> \"$RNGIT_TEST_MEDIA_ARGS\"\nexec '{}' \"$@\"\n",
            backend_executable.replace('\'', "'\\''")
        ),
    )?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&backend_probe, fs::Permissions::from_mode(0o755))?;
    let mut search_paths = vec![backend_dir];
    search_paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(search_paths).map_err(io::Error::other)?;
    let port = free_port()?;
    let identity_seed = format!("rngit-configured-{backend}-link-regression");
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            &identity_seed,
            "--media-quality",
            "37",
            "--media-max-dimension",
            "1",
            "--silent",
        ])
        .env("RNGIT_MEDIA_BACKEND", &backend)
        .env("RNGIT_TEST_WEBP_BACKEND", &backend)
        .env("RNGIT_TEST_MEDIA_ARGS", &args_path)
        .env("PATH", path)
        .env("TMPDIR", &media_temp)
        .env("TEMP", &media_temp)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, &identity_seed)?;
        let config_dir = temp.path().join("python-client");
        fs::create_dir_all(&config_dir)?;
        write_python_config(&config_dir, port)?;
        let webp_path = temp.path().join("response.webp");
        let output = Command::new(python_bin())
            .arg("-c")
            .arg(PYTHON_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .arg(&media_temp)
            .arg(&webp_path)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "configured {backend} Link client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(io::Error::other)?;
        assert_eq!(response["converted_name"], "valid.webp");
        assert_eq!(response["converted_header_valid"], true);
        assert_eq!(response["conversion_temp_directories_while_link_open"], 1);
        assert_eq!(response["fallback_name"], "image.png");
        assert_eq!(response["fallback_size"], 8192);
        assert_eq!(
            response["fallback_sha256"],
            "f8e920545e99cdc9bbc2650eb8282344e8971a7ff0c397c91355d0fcaf6c61fa"
        );
        let argv_file = fs::read_to_string(&args_path)?;
        let argv = argv_file
            .split_once("--BEGIN--\n")
            .map(|(_, args)| {
                args.lines().take_while(|line| *line != "--BEGIN--").collect::<Vec<_>>()
            })
            .ok_or_else(|| io::Error::other("backend did not record its first invocation"))?;
        assert!(argv.windows(2).any(|pair| pair == ["-quality", "37"]), "argv: {argv:?}");
        let dimension_option_present = if matches!(backend.as_str(), "magick" | "convert" | "gm") {
            argv.windows(2).any(|pair| pair == ["-resize", "1x1>"])
        } else {
            argv.windows(2).any(|pair| {
                pair[0] == "-vf"
                    && pair[1]
                        == "scale='min(iw,1)':'min(ih,1)':force_original_aspect_ratio=decrease"
            })
        };
        assert!(dimension_option_present, "argv: {argv:?}");

        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name,width,height",
                "-of",
                "json",
            ])
            .arg(&webp_path)
            .output()?;
        if !probe.status.success() {
            return Err(io::Error::other(format!(
                "ffprobe rejected configured WebP response: {}",
                String::from_utf8_lossy(&probe.stderr)
            )));
        }
        let decoded: serde_json::Value =
            serde_json::from_slice(&probe.stdout).map_err(io::Error::other)?;
        assert_eq!(decoded["streams"][0]["codec_name"], "webp");
        assert_eq!(decoded["streams"][0]["width"], 1);
        assert_eq!(decoded["streams"][0]["height"], 1);
        wait_for_empty_directory(&media_temp)?;
        Ok(())
    })();
    if server.try_wait()?.is_none() {
        let _ = server.kill();
    }
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

config_dir, destination_hex, media_temp_dir, webp_path = sys.argv[1:5]
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

def request(path):
    finished = threading.Event()
    result = {}
    def response(receipt):
        payload = receipt.response.read()
        result["name"] = receipt.metadata.get("name", b"").decode("utf-8") if receipt.metadata else None
        result["size"] = len(payload)
        result["sha256"] = hashlib.sha256(payload).hexdigest()
        result["header_valid"] = payload[:4] == b"RIFF" and payload[8:12] == b"WEBP"
        if result["name"] == "valid.webp":
            with open(webp_path, "wb") as output:
                output.write(payload)
        finished.set()
    def failed(_receipt):
        result["failed"] = True
        finished.set()
    receipt = link.request("/media", {"key": b"configured-ffmpeg", "path": path}, response_callback=response, failed_callback=failed, timeout=20)
    if receipt is False or not finished.wait(25):
        raise RuntimeError("media Resource did not complete: " + path)
    if result.get("failed"):
        raise RuntimeError("media Resource failed instead of returning the production fallback/response")
    return result

converted = request("/media/group/repo/HEAD/valid.png")
deadline = time.monotonic() + 5
while time.monotonic() < deadline and not os.listdir(media_temp_dir):
    time.sleep(0.02)
temp_count = len([name for name in os.listdir(media_temp_dir) if name.startswith("rngit-media-")])
if temp_count != 1:
    raise RuntimeError("converted-media directory was not retained for the active Link")
fallback = request("/media/group/repo/HEAD/image.png")
link.teardown()
print(json.dumps({
    "converted_name": converted["name"],
    "converted_header_valid": converted["header_valid"],
    "conversion_temp_directories_while_link_open": temp_count,
    "fallback_name": fallback["name"],
    "fallback_size": fallback["size"],
    "fallback_sha256": fallback["sha256"],
}, sort_keys=True))
"#;

const PNG_FIXTURE_WRITER: &str = r#"
import binascii, struct, sys, zlib
def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", binascii.crc32(kind + data) & 0xffffffff)
width, height = 8, 4
rows = b"".join(b"\0" + bytes(channel for x in range(width) for channel in (x * 29, y * 53, (x + y) * 17)) for y in range(height))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
"#;
