use std::fs::{self, File};
use std::io;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
            return Err(io::Error::other(format!("Rust rncp listener exited early: {status}")));
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
    let hash = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if hash.is_empty() {
        return Err(io::Error::other("Python identity setup returned an empty hash"));
    }
    Ok(hash)
}

fn resource_progress(resources: &Path, expected_size: u64) -> io::Result<Option<(PathBuf, u64)>> {
    if !resources.is_dir() {
        return Ok(None);
    }
    for entry in fs::read_dir(resources)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.file_name().to_string_lossy().ends_with(".meta") {
            continue;
        }
        let size = entry.metadata()?.len();
        if size > 0 && size < expected_size {
            return Ok(Some((entry.path(), size)));
        }
    }
    Ok(None)
}

#[cfg(unix)]
#[test]
#[ignore = "requires the frozen Python Reticulum checkout"]
fn rncp_pinned_python_fetch_ctrl_c_records_active_resource_cancellation() -> io::Result<()> {
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
        Command::new("git").args(["-C"]).arg(&repo).args(["rev-parse", "HEAD"]).output()?;
    if !reference_revision.status.success()
        || String::from_utf8_lossy(&reference_revision.stdout).trim()
            != "99de23c040d507e3fefca19e87b182302902725d"
    {
        return Err(io::Error::other(format!(
            "expected frozen Reticulum reference 99de23c040d507e3fefca19e87b182302902725d, got {}",
            String::from_utf8_lossy(&reference_revision.stdout).trim()
        )));
    }

    let port = free_port()?;
    let rust_root = temp.path().join("rust-listener");
    let python_config = temp.path().join("python-client-config");
    let save_root = temp.path().join("python-save-root");
    fs::create_dir_all(&rust_root)?;
    fs::create_dir_all(&python_config)?;
    fs::create_dir_all(&save_root)?;
    fs::write(
        python_config.join("config"),
        format!(
            "[reticulum]\nenable_transport = no\nshare_instance = no\n\n[logging]\nloglevel = 0\n\n[interfaces]\n[[TCP Client Interface]]\ntype = TCPClientInterface\nenabled = yes\ntarget_host = 127.0.0.1\ntarget_port = {port}\n"
        ),
    )?;
    let python_identity = python_config.join("identity");
    let _python_identity_hash = python_identity_hash(&python, &python_identity, &repo)?;
    let identity_seed = "rncp-python-cancel-rust-listener";
    let identity_output = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .args(["--print-identity", "--identity-seed", identity_seed])
        .output()?;
    if !identity_output.status.success() {
        return Err(io::Error::other("Rust rncp listener identity query failed"));
    }
    let destination = String::from_utf8_lossy(&identity_output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Listening on : "))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("Rust rncp listener identity was omitted"))?;

    let payload_size = 48 * 1024 * 1024_u64;
    let source = rust_root.join("cancel-fetch.bin");
    let mut source_file = File::create(&source)?;
    let chunk = (0..64 * 1024)
        .map(|index| (index as u8).wrapping_mul(73).wrapping_add(19))
        .collect::<Vec<_>>();
    for _ in 0..(payload_size as usize / chunk.len()) {
        std::io::Write::write_all(&mut source_file, &chunk)?;
    }
    drop(source_file);

    let listener_stderr = temp.path().join("rust-listener.stderr");
    let mut listener = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .args([
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--no-auth",
            "--allow-fetch",
            "--no-compress",
            "--save",
        ])
        .arg(&rust_root)
        .args(["--identity-seed", identity_seed, "--timeout", "60"])
        .current_dir(&rust_root)
        .stdout(Stdio::null())
        .stderr(Stdio::from(File::create(&listener_stderr)?))
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let python_stdout = temp.path().join("python-fetch.stdout");
        let mut client = Command::new(&python)
            .arg(&script)
            .arg("cancel-fetch.bin")
            .arg(&destination)
            .args(["--fetch", "--config"])
            .arg(&python_config)
            .arg("-i")
            .arg(&python_identity)
            .arg("--save")
            .arg(&save_root)
            .args(["-C", "-w", "60"])
            .env("PYTHONPATH", &repo)
            .env("PYTHONUNBUFFERED", "1")
            .stdout(Stdio::from(File::create(&python_stdout)?))
            .stderr(Stdio::from(File::create(temp.path().join("python-fetch.stderr"))?))
            .spawn()?;

        let resources = python_config.join("storage/resources");
        let deadline = Instant::now() + Duration::from_secs(40);
        let partial_path = loop {
            if let Some((path, size)) = resource_progress(&resources, payload_size)? {
                let progress_output = fs::read_to_string(&python_stdout).unwrap_or_default();
                if size > 0 && progress_output.contains("Transferring file") {
                    break path;
                }
            }
            if let Some(status) = client.try_wait()? {
                return Err(io::Error::other(format!(
                    "pinned Python fetch exited before an active partial Resource was observed: {status}"
                )));
            }
            if Instant::now() >= deadline {
                let _ = client.kill();
                let status = client.wait()?;
                let stdout = fs::read_to_string(&python_stdout).unwrap_or_default();
                let stderr =
                    fs::read_to_string(temp.path().join("python-fetch.stderr")).unwrap_or_default();
                let listener_output = fs::read_to_string(&listener_stderr).unwrap_or_default();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("pinned Python fetch never wrote a partial Resource before timeout; child={status}; stdout={stdout:?}; stderr={stderr:?}; Rust listener stderr={listener_output:?}"),
                ));
            }
            thread::sleep(Duration::from_millis(10));
        };

        let signal = Command::new("kill").args(["-INT", &client.id().to_string()]).status()?;
        if !signal.success() {
            let _ = client.kill();
            let _ = client.wait();
            return Err(io::Error::other("failed to send SIGINT to active pinned Python fetch"));
        }
        let status = client.wait()?;
        assert_eq!(status.code(), Some(0), "pinned rncp SIGINT exit status changed");
        let stdout = fs::read_to_string(&python_stdout)?;
        assert!(
            stdout.contains("Requesting file from remote"),
            "fetch request phase missing: {stdout:?}"
        );
        assert!(stdout.contains("Transferring file"), "active transfer output missing: {stdout:?}");
        assert!(
            !stdout.contains("Resource failed")
                && !stdout.contains("The transfer failed")
                && !stdout.contains("fetched from"),
            "pinned rncp unexpectedly reported a terminal fetch result: {stdout:?}"
        );
        assert!(
            !save_root.join("cancel-fetch.bin").exists(),
            "completed fetch file remained after cancellation"
        );
        let staged = fs::metadata(&partial_path)?.len();
        assert!(
            staged > 0 && staged < payload_size,
            "staged Resource was not partial: {staged} bytes"
        );
        assert!(
            fs::read_dir(&save_root)?.next().is_none(),
            "Python save directory was not empty after cancellation"
        );
        let sender_deadline = Instant::now() + Duration::from_secs(5);
        let listener_log = loop {
            let log = fs::read_to_string(&listener_stderr)?;
            if log.contains("rncp: outgoing Resource failed") {
                break log;
            }
            if Instant::now() >= sender_deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("Rust fetch sender did not report the failed outbound Resource; stderr: {log:?}"),
                ));
            }
            thread::sleep(Duration::from_millis(25));
        };
        eprintln!(
            "pinned rncp cancellation transcript: status={}, output contains Requesting file from remote / Transferring file and no terminal result; completed file absent; staged partial remains {} bytes at {}; Rust sender stderr: {}",
            status,
            staged,
            partial_path.display(),
            listener_log.trim()
        );
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}
