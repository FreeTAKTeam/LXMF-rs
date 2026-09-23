use std::fs::{self, File};
use std::io;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const REQUEST_STARTED: &str = "Requesting file from remote";
const SAVE_FAILED: &str = "An error occurred while saving received resource:";

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
        .env("PYTHONPATH", spec.repo)
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
                "Python rncp exited before emitting {marker:?}: {status}\nstdout:\n{output}"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("Python rncp did not emit {marker:?}; stdout:\n{output}"),
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn observe_python_fetch_save_failure(
    child: &mut Child,
    save_root: &Path,
    stdout: &Path,
) -> io::Result<String> {
    let result = (|| {
        wait_for_output(child, stdout, REQUEST_STARTED)?;
        fs::remove_dir(save_root)?;
        fs::write(save_root, b"replace the validated save directory")?;
        let output = wait_for_output(child, stdout, SAVE_FAILED)?;
        thread::sleep(Duration::from_millis(200));
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "pinned Python fetch unexpectedly resolved after its save callback failed ({status})\nstdout:\n{output}"
            )));
        }
        Ok(output)
    })();
    if child.try_wait()?.is_none() {
        child.kill()?;
    }
    child.wait()?;
    result
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

    let listener_root = temp.path().join("rust-listener");
    let config = temp.path().join("python-client-config");
    let save_root = temp.path().join("python-save-root");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&config)?;
    fs::create_dir_all(&save_root)?;
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
    let mut listener = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .args([
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--allowed-identity",
            &allowed_identity,
            "--save",
        ])
        .arg(&listener_root)
        .args(["--identity-seed", identity_seed, "--allow-fetch", "--silent", "--timeout", "30"])
        .current_dir(&listener_root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
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
        fs::write(&source, payload)?;
        let stdout = temp.path().join("python-fetch.stdout");
        let stderr = temp.path().join("python-fetch.stderr");
        let mut client = python_fetch_destination(PythonFetchSpec {
            python: &python,
            repo: &repo,
            script: &script,
            config: &config,
            identity: &python_identity,
            remote_file: "fetch-failure.bin",
            destination: &destination,
            save_root: &save_root,
            stdout_path: &stdout,
            stderr_path: &stderr,
        })?;
        observe_python_fetch_save_failure(&mut client, &save_root, &stdout)
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    let output = result?;
    assert!(output.contains(SAVE_FAILED), "Python fetch callback error was not reported: {output}");
    assert!(
        !output.contains("fetch-failure.bin fetched from"),
        "Python fetch reported success despite the save error: {output}"
    );
    assert!(
        save_root.is_file(),
        "the callback failure was not caused by the replaced save directory"
    );
    Ok(())
}
