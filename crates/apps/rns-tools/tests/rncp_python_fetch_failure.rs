use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const REQUEST_STARTED: &str = "Requesting file from remote";
const SAVE_FAILED: &str = "An error occurred while saving received resource:";
const RUST_OUTBOUND_COMPLETE: &str = "rncp: outgoing Resource complete (";
const SAVE_ROOT_COLLISION: &[u8] = b"replace the validated save directory";
const RESOURCE_OBSERVER: &str = r#"
import hashlib
import os
import shutil

_original_move = shutil.move

def _observe_resource_then_move(source, destination, *args, **kwargs):
    source_path = os.path.abspath(source)
    resource_marker = os.sep + "storage" + os.sep + "resources" + os.sep
    if resource_marker in source_path and os.path.isfile(source_path):
        with open(source_path, "rb") as resource_file:
            payload = resource_file.read()
        observation_path = os.environ["RNCP_RESOURCE_OBSERVATION"]
        with open(observation_path, "w", encoding="utf-8") as observation:
            observation.write(f"resource_hash={os.path.basename(source_path)}\n")
            observation.write(f"size={len(payload)}\n")
            observation.write(f"sha256={hashlib.sha256(payload).hexdigest()}\n")
    return _original_move(source, destination, *args, **kwargs)

shutil.move = _observe_resource_then_move
"#;

fn free_port() -> io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn python_repo() -> PathBuf {
    std::env::var_os("RETICULUM_PY_REPO").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.tmp/python-refs/Reticulum")
    })
}

fn wait_for_port(port: u16, child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let Ok(stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            drop(stream);
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "Rust rncp listener exited before accepting clients: {status}"
            )));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "Rust rncp listener did not open its TCP port"))
}

fn python_identity_hash(python: &str, identity: &Path, repo: &Path) -> io::Result<String> {
    let output = Command::new(python)
        .arg("-c")
        .arg("import os,sys,RNS; i=RNS.Identity.from_file(sys.argv[1]) if os.path.isfile(sys.argv[1]) else RNS.Identity(); i.to_file(sys.argv[1]); print(i.hash.hex())")
        .arg(identity)
        .env("PYTHONPATH", repo)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Python identity setup failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let identity_hash = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if identity_hash.is_empty() {
        return Err(io::Error::other("Python identity setup returned an empty hash"));
    }
    Ok(identity_hash)
}

struct PythonFetchSpec<'a> {
    python: &'a str,
    repo: &'a Path,
    hook_root: &'a Path,
    resource_observation: &'a Path,
    script: &'a Path,
    config: &'a Path,
    identity: &'a Path,
    remote_file: &'a str,
    destination: &'a str,
    save_root: &'a Path,
    stdout_path: &'a Path,
    stderr_path: &'a Path,
}

fn python_fetch_destination(spec: PythonFetchSpec<'_>) -> io::Result<Child> {
    let python_path = std::env::join_paths([spec.hook_root.as_os_str(), spec.repo.as_os_str()])
        .map_err(|error| io::Error::other(format!("invalid Python import path: {error}")))?;
    Command::new(spec.python)
        .arg(spec.script)
        .arg(spec.remote_file)
        .arg(spec.destination)
        .args(["--fetch", "--config"])
        .arg(spec.config)
        .arg("-i")
        .arg(spec.identity)
        .arg("-s")
        .arg(spec.save_root)
        .args(["-O", "-C", "-w", "30"])
        .env("PYTHONPATH", python_path)
        .env("RNCP_RESOURCE_OBSERVATION", spec.resource_observation)
        .env("PYTHONUNBUFFERED", "1")
        .stdout(Stdio::from(File::create(spec.stdout_path)?))
        .stderr(Stdio::from(File::create(spec.stderr_path)?))
        .spawn()
}

fn wait_for_output(child: &mut Child, stdout: &Path, marker: &str) -> io::Result<String> {
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let output = fs::read_to_string(stdout)?;
        if output.contains(marker) {
            return Ok(output);
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "child process exited before emitting {marker:?}: {status}\nstdout:\n{output}"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("child process did not emit {marker:?}; stdout:\n{output}"),
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

struct PythonFetchObservation {
    stdout: String,
    stderr: String,
    status_before_cleanup: Option<ExitStatus>,
    status_after_cleanup: ExitStatus,
}

fn observe_python_fetch_save_failure(
    child: &mut Child,
    listener: &mut Child,
    save_root: &Path,
    stdout: &Path,
    stderr: &Path,
    listener_stderr: &Path,
) -> io::Result<PythonFetchObservation> {
    let result = (|| {
        wait_for_output(child, stdout, REQUEST_STARTED)?;
        fs::remove_dir(save_root)?;
        fs::write(save_root, SAVE_ROOT_COLLISION)?;
        wait_for_output(child, stdout, SAVE_FAILED)?;
        let listener_output = wait_for_output(listener, listener_stderr, RUST_OUTBOUND_COMPLETE)?;
        thread::sleep(Duration::from_millis(200));
        let status_before_cleanup = child.try_wait()?;
        let output = fs::read_to_string(stdout)?;
        let error_output = fs::read_to_string(stderr)?;
        if status_before_cleanup.is_some() {
            return Err(io::Error::other(format!(
                "pinned Python fetch unexpectedly exited after its save callback failed; status={status_before_cleanup:?}\nstdout:\n{output}\nstderr:\n{error_output}\nRust listener stderr:\n{listener_output}"
            )));
        }
        Ok((output, error_output, status_before_cleanup))
    })();
    if child.try_wait()?.is_none() {
        child.kill()?;
    }
    let status_after_cleanup = child.wait()?;
    let (stdout, stderr, status_before_cleanup) = result?;
    Ok(PythonFetchObservation { stdout, stderr, status_before_cleanup, status_after_cleanup })
}

#[test]
#[ignore = "requires the pinned Python Reticulum checkout"]
fn rncp_python_fetch_client_save_error_is_reported_but_never_resolved() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let repo = python_repo();
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_owned());
    let script = repo.join("RNS/Utilities/rncp.py");
    if !script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rncp script not found: {}", script.display()),
        ));
    }
    let reference_revision =
        Command::new("git").arg("-C").arg(&repo).args(["rev-parse", "HEAD"]).output()?;
    if !reference_revision.status.success()
        || String::from_utf8_lossy(&reference_revision.stdout).trim()
            != "99de23c040d507e3fefca19e87b182302902725d"
    {
        return Err(io::Error::other(format!(
            "expected frozen Reticulum reference 99de23c040d507e3fefca19e87b182302902725d, got {}",
            String::from_utf8_lossy(&reference_revision.stdout).trim()
        )));
    }

    let listener_root = temp.path().join("rust-listener");
    let config = temp.path().join("python-client-config");
    let save_root = temp.path().join("python-save-root");
    let hook_root = temp.path().join("python-hooks");
    let resource_observation = temp.path().join("python-resource-observation.txt");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&config)?;
    fs::create_dir_all(&save_root)?;
    fs::create_dir_all(&hook_root)?;
    fs::write(hook_root.join("sitecustomize.py"), RESOURCE_OBSERVER)?;
    let port = free_port()?;
    fs::write(
        config.join("config"),
        format!(
            "[reticulum]\nenable_transport = no\nshare_instance = no\n\n[logging]\nloglevel = 0\n\n[interfaces]\n[[TCP Client Interface]]\ntype = TCPClientInterface\nenabled = yes\ntarget_host = 127.0.0.1\ntarget_port = {port}\n"
        ),
    )?;
    let python_identity = config.join("identity");
    let allowed_identity = python_identity_hash(&python, &python_identity, &repo)?;
    let identity_seed = "rncp-python-fetch-save-failure-listener";
    let listener_stdout = temp.path().join("rust-listener.stdout");
    let listener_stderr = temp.path().join("rust-listener.stderr");
    let mut listener = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .args([
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--allowed-identity",
            &allowed_identity,
            "--save",
        ])
        .arg(&listener_root)
        .args(["--identity-seed", identity_seed, "--allow-fetch", "--timeout", "30"])
        .current_dir(&listener_root)
        .stdout(Stdio::from(File::create(&listener_stdout)?))
        .stderr(Stdio::from(File::create(&listener_stderr)?))
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let identity_output = Command::new(env!("CARGO_BIN_EXE_rncp"))
            .args(["--print-identity", "--identity-seed", identity_seed])
            .output()?;
        if !identity_output.status.success() {
            return Err(io::Error::other(format!(
                "Rust rncp identity query failed: {}\n{}",
                identity_output.status,
                String::from_utf8_lossy(&identity_output.stderr)
            )));
        }
        let destination = String::from_utf8_lossy(&identity_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("Rust rncp identity query omitted destination"))?
            .to_owned();

        let source = listener_root.join("fetch-failure.bin");
        let payload = (0..2 * 1024 * 1024)
            .map(|index| (index as u8).wrapping_mul(31).wrapping_add(9))
            .collect::<Vec<_>>();
        fs::write(&source, &payload)?;
        let stdout = temp.path().join("python-fetch.stdout");
        let stderr = temp.path().join("python-fetch.stderr");
        let mut client = python_fetch_destination(PythonFetchSpec {
            python: &python,
            repo: &repo,
            hook_root: &hook_root,
            resource_observation: &resource_observation,
            script: &script,
            config: &config,
            identity: &python_identity,
            remote_file: "fetch-failure.bin",
            destination: &destination,
            save_root: &save_root,
            stdout_path: &stdout,
            stderr_path: &stderr,
        })?;
        let observation = observe_python_fetch_save_failure(
            &mut client,
            &mut listener,
            &save_root,
            &stdout,
            &stderr,
            &listener_stderr,
        )?;
        let listener_stdout_output = fs::read_to_string(&listener_stdout)?;
        let listener_stderr_output = fs::read_to_string(&listener_stderr)?;
        let payload_observation = fs::read_to_string(&resource_observation)?;
        eprintln!(
            "pinned rncp fetch save-failure transcript: Python status before cleanup={:?}, cleanup status={}; callback payload evidence={:?}; Python stdout={:?}; Python stderr={:?}; Rust listener stdout={:?}; Rust listener stderr={:?}",
            observation.status_before_cleanup,
            observation.status_after_cleanup,
            payload_observation,
            observation.stdout,
            observation.stderr,
            listener_stdout_output,
            listener_stderr_output
        );
        Ok((observation, listener_stderr_output, payload, payload_observation))
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    let (observation, listener_stderr_output, payload, payload_observation) = result?;
    let output = observation.stdout;
    assert!(output.contains(SAVE_FAILED), "Python fetch callback error was not reported: {output}");
    assert!(
        output.contains("Transfer complete"),
        "pinned Python no longer reports Resource completion after its save callback fails: {output}"
    );
    assert!(
        !output.contains("fetch-failure.bin fetched from"),
        "Python fetch reported success despite the save error: {output}"
    );
    assert!(
        observation.status_before_cleanup.is_none(),
        "Python fetch had already exited before cleanup: {:?}",
        observation.status_before_cleanup
    );
    assert!(
        !observation.status_after_cleanup.success(),
        "Python fetch unexpectedly succeeded after test cleanup: {}",
        observation.status_after_cleanup
    );
    let python_resource_hash = payload_observation
        .lines()
        .find_map(|line| line.strip_prefix("resource_hash="))
        .ok_or_else(|| io::Error::other("Python Resource hash observation is missing"))?;
    let expected_resource_observation = format!(
        "resource_hash={python_resource_hash}\nsize={}\nsha256={}\n",
        payload.len(),
        hex::encode(Sha256::digest(&payload))
    );
    assert_eq!(
        payload_observation, expected_resource_observation,
        "pinned Python save callback did not see the complete expected payload"
    );
    let listener_resource_hash = listener_stderr_output.lines().find_map(|line| {
        line.strip_prefix(RUST_OUTBOUND_COMPLETE).and_then(|hash| hash.strip_suffix(')'))
    });
    assert_eq!(
        listener_resource_hash,
        Some(python_resource_hash),
        "Rust listener's OutboundComplete event was not for the Resource received by Python; listener stderr={listener_stderr_output:?}"
    );
    assert!(
        save_root.is_file(),
        "the callback failure was not caused by the replaced save directory"
    );
    assert_eq!(fs::read(&save_root)?, SAVE_ROOT_COLLISION);
    assert!(!save_root.join("fetch-failure.bin").exists());
    let resources = config.join("storage/resources");
    assert!(resources.is_dir(), "Python Reticulum resource storage was not initialized");
    assert_eq!(
        fs::read_dir(resources)?.count(),
        0,
        "completed Python Resource left a partial/staged storage file"
    );
    Ok(())
}
