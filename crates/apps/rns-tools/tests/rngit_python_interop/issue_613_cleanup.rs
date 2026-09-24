use super::*;

#[test]
#[ignore = "requires local pinned Python Reticulum checkout and ffmpeg"]
fn rngit_cancels_in_flight_media_resource_on_python_link_teardown() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let media_temp_directory = temp.path().join("media-temp");
    fs::create_dir(&media_temp_directory)?;
    let python_repo = python_repo();
    if !python_repo.join("RNS/Link.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }
    if !Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success()
    {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "ffmpeg is required for media cancellation",
        ));
    }

    let port = free_port()?;
    let identity_seed = "rngit-python-media-cancel";
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
        let output = run_python_media_cancellation_client(
            &python_repo,
            &config_dir,
            &identity,
            &destination,
            &media_temp_directory,
        )?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python media cancellation client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("\"progress_observed\": true"),
            "no in-flight progress signal: {stdout}"
        );
        assert!(
            stdout.contains("\"response_completed\": false"),
            "cancelled media falsely completed: {stdout}"
        );
        assert!(
            stdout.contains("\"receiver_resources_after_close\": 0"),
            "Python Link retained Resource state: {stdout}"
        );
        #[cfg(target_os = "linux")]
        assert_no_child_processes(server.id())?;
        wait_for_empty_directory(&media_temp_directory)
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

#[cfg(unix)]
#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn rngit_cleans_media_after_response_fails_on_abrupt_client_exit() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let media_temp_directory = temp.path().join("rust-media-temp");
    let converter_directory = temp.path().join("converter");
    let release_conversion = temp.path().join("release-conversion");
    fs::create_dir(&media_temp_directory)?;
    fs::create_dir(&converter_directory)?;
    let converter = converter_directory.join("ffmpeg");
    fs::write(
        &converter,
        b"#!/bin/sh\ncat >/dev/null\nwhile [ ! -e \"$RNGIT_TEST_CONVERT_RELEASE\" ]; do sleep 0.01; done\nprintf 'RIFF\\036\\000\\000\\000WEBPVP8X\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000'\n",
    )?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&converter, fs::Permissions::from_mode(0o755))?;
    let port = free_port()?;
    let identity_seed = "rngit-rust-abrupt-media-exit-trace";
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            identity_seed,
        ])
        .env("RNGIT_MEDIA_BACKEND", "ffmpeg")
        .env("RNGIT_TEST_CONVERT_RELEASE", &release_conversion)
        .env("TMPDIR", &media_temp_directory)
        .env("TEMP", &media_temp_directory)
        .env("PATH", {
            let mut paths = vec![converter_directory];
            paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
            std::env::join_paths(paths).map_err(io::Error::other)?
        })
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, identity_seed)?;
        let client_config = temp.path().join("python-client");
        fs::create_dir(&client_config)?;
        write_python_config(&client_config, port)?;
        let client = Command::new(python_bin())
            .arg("-c")
            .arg(RUST_ABRUPT_EXIT_CLIENT)
            .arg(&client_config)
            .arg(&destination)
            .arg(&media_temp_directory)
            .env("PYTHONPATH", python_repo())
            .output()?;
        if !client.status.success() {
            return Err(io::Error::other(format!(
                "abrupt-exit client against Rust server failed: {}\nstdout:\n{}\nstderr:\n{}",
                client.status,
                String::from_utf8_lossy(&client.stdout),
                String::from_utf8_lossy(&client.stderr)
            )));
        }
        assert!(
            String::from_utf8_lossy(&client.stdout).contains("temporary_media_observed"),
            "client did not exit after observing the active media directory"
        );
        fs::write(&release_conversion, b"complete")?;

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if fs::read_dir(&media_temp_directory)?.next().transpose()?.is_none() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "rngit retained conversion data after the response send detected the abrupt client exit",
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

#[cfg(unix)]
const RUST_ABRUPT_EXIT_CLIENT: &str = r#"
import os
import sys
import time
import threading
import RNS

config_dir, destination_hex, media_temp_directory = sys.argv[1:4]
RNS.Reticulum(configdir=config_dir, loglevel=0)
destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve page destination")
remote_identity = RNS.Identity.recall(destination_hash)
if remote_identity is None:
    raise RuntimeError("could not recall page identity")
destination = RNS.Destination(remote_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node")
ready = threading.Event()
link = RNS.Link(destination)
link.set_link_established_callback(lambda _link: ready.set())
if not ready.wait(30):
    raise RuntimeError("Link establishment timed out")
if link.request("/media", {"key": b"abrupt-exit", "path": "/media/group/repo/HEAD/valid.png"}) is False:
    raise RuntimeError("media request was not sent")
deadline = time.monotonic() + 20
while time.monotonic() < deadline:
    if os.listdir(media_temp_directory):
        os.write(1, b"temporary_media_observed\n")
        os._exit(0)
    time.sleep(0.01)
raise RuntimeError("conversion temporary directory was never observed")
"#;

fn run_python_media_cancellation_client(
    repo: &Path,
    config_dir: &Path,
    identity_path: &Path,
    destination: &str,
    media_temp_directory: &Path,
) -> io::Result<Output> {
    const CLIENT: &str = r#"
import json
import os
import sys
import threading
import RNS

config_dir, identity_path, destination_hex, media_temp_directory = sys.argv[1:5]
RNS.Reticulum(configdir=config_dir, loglevel=0)
identity = RNS.Identity.from_file(identity_path) if os.path.isfile(identity_path) else RNS.Identity()
if not os.path.isfile(identity_path):
    identity.to_file(identity_path)
destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
remote_identity = RNS.Identity.recall(destination_hash)
if remote_identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(remote_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node")
ready = threading.Event()
failed = []
def established(link):
    link.identify(identity)
    ready.set()
def closed(link):
    if not ready.is_set():
        failed.append("link closed before activation")
        ready.set()
link = RNS.Link(destination)
link.set_link_established_callback(established)
link.set_link_closed_callback(closed)
if not ready.wait(30):
    raise RuntimeError("link establishment timed out")
if failed:
    raise RuntimeError(failed[0])

progress_observed = threading.Event()
release_progress_callback = threading.Event()
progress_callback_returned = threading.Event()
response_completed = threading.Event()
def progress(receipt):
    if 0.0 < receipt.progress < 1.0:
        progress_observed.set()
        if not release_progress_callback.wait(30):
            raise RuntimeError("test did not release the synchronized progress callback")
        progress_callback_returned.set()
def response(receipt):
    response_completed.set()
def failed_response(receipt):
    pass
receipt = link.request(
    "/media",
    {"key": b"rngit-python-interop", "path": "/media/group/repo/HEAD/large.png"},
    response_callback=response,
    failed_callback=failed_response,
    progress_callback=progress,
    timeout=60,
)
if receipt is False:
    raise RuntimeError("media request was not sent")
if not progress_observed.wait(30):
    raise RuntimeError("no response Resource progress callback was observed")
if response_completed.is_set():
    raise RuntimeError("large media response completed before cancellation")
link.teardown()
release_progress_callback.set()
if not progress_callback_returned.wait(5):
    raise RuntimeError("response progress callback did not return after Link teardown")
if link.incoming_resources:
    raise RuntimeError(f"Link retained {len(link.incoming_resources)} incoming Resources after teardown")
if response_completed.is_set():
    raise RuntimeError("cancelled media response invoked the completion callback")
resource_files = os.listdir(RNS.Reticulum.resourcepath)
if resource_files:
    raise RuntimeError(f"cancelled response left Resource files: {resource_files}")
print(json.dumps({
    "progress_observed": True,
    "response_completed": response_completed.is_set(),
    "receiver_resources_after_close": len(link.incoming_resources),
    "receiver_resource_files_after_close": resource_files,
    "media_temp_entries_after_close": os.listdir(media_temp_directory),
}, sort_keys=True))
"#;
    Command::new(python_bin())
        .arg("-c")
        .arg(CLIENT)
        .arg(config_dir)
        .arg(identity_path)
        .arg(destination)
        .arg(media_temp_directory)
        .env("PYTHONPATH", repo)
        .output()
}

#[cfg(target_os = "linux")]
fn assert_no_child_processes(server_pid: u32) -> io::Result<()> {
    let children = fs::read_to_string(format!("/proc/{server_pid}/task/{server_pid}/children"))?;
    if children.trim().is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(format!("rngit still has child processes: {children}")))
    }
}

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_serves_pages_and_media_to_pinned_python_client() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let media_temp_directory = temp.path().join("media-temp");
    fs::create_dir(&media_temp_directory)?;
    let python_repo = python_repo();
    if !python_repo.join("RNS/Link.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }

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
        .env("TMPDIR", &media_temp_directory)
        .env("TEMP", &media_temp_directory)
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
        assert!(
            stdout.contains("\"media_temp_directories_during_link\": 1"),
            "conversion temporary data was not observed before disconnect: {stdout}"
        );
        wait_for_empty_directory(&media_temp_directory)?;

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
