#![cfg(unix)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

static PYTHON_INTEROP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn free_port() -> io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn wait_for_port(port: u16, child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!("rnsh listener exited early: {status}")));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "rnsh listener did not open its port"))
}

fn python_repo() -> PathBuf {
    let configured = std::env::var_os("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".tmp/python-refs/Reticulum"));
    if configured.is_absolute() {
        configured
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").join(configured)
    }
}

fn python_bin() -> String {
    std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string())
}

fn write_python_config(dir: &Path, port: u16) -> io::Result<()> {
    fs::write(
        dir.join("config"),
        format!(
            "[reticulum]\n\
             enable_transport = no\n\
             share_instance = no\n\
             \n\
             [logging]\n\
             loglevel = 0\n\
             \n\
             [interfaces]\n\
             [[TCP Client Interface]]\n\
             type = TCPClientInterface\n\
             enabled = yes\n\
             target_host = 127.0.0.1\n\
             target_port = {port}\n"
        ),
    )
}

fn run_with_timeout(mut child: Child, timeout: Duration) -> io::Result<Output> {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let output = child.wait_with_output()?;
            return Err(io::Error::other(format!(
                "Python rnsh timed out\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn pinned_python_rnsh_initiator_executes_a_command_on_rust_listener() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let server_root = temp.path().join("server-root");
    let server_identity = temp.path().join("server-identity");
    let python_rnsh_config = temp.path().join("python-rnsh");
    let python_rns_config = temp.path().join("python-rns");
    let python_identity = temp.path().join("python-identity");
    for directory in [&server_root, &python_rnsh_config, &python_rns_config] {
        fs::create_dir_all(directory)?;
    }

    let repo = python_repo();
    let script = repo.join("RNS/Utilities/rnsh/rnsh.py");
    if !script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rnsh script not found: {}", script.display()),
        ));
    }
    let python = python_bin();
    let port = free_port()?;
    write_python_config(&python_rns_config, port)?;

    let binary = env!("CARGO_BIN_EXE_rnsh");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--announce", "0", "--no-auth", "--root"])
        .arg(&server_root)
        .args(["--identity", server_identity.to_str().expect("identity path")])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args([
                "--print-identity",
                "--identity",
                server_identity.to_str().expect("identity path"),
            ])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other(format!(
                "rnsh identity query failed: {}",
                String::from_utf8_lossy(&destination_output.stderr)
            )));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("rnsh identity query omitted destination"))?
            .to_owned();

        let mut python = Command::new(&python)
            .arg(&script)
            .arg(&destination)
            .args(["--config", python_rnsh_config.to_str().expect("config path")])
            .args(["--rnsconfig", python_rns_config.to_str().expect("RNS config path")])
            .args(["--identity", python_identity.to_str().expect("identity path")])
            .args(["--quiet", "--mirror"])
            .args(["--", "/bin/echo", "python-rnsh-to-rust"])
            .env("PYTHONPATH", &repo)
            .current_dir(temp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        drop(python.stdin.take());
        let output = match run_with_timeout(python, Duration::from_secs(45)) {
            Ok(output) => output,
            Err(error) => {
                let log = fs::read_to_string(python_rnsh_config.join("logfile.initiator"))
                    .unwrap_or_else(|log_error| {
                        format!("<could not read Python log: {log_error}>")
                    });
                return Err(io::Error::other(format!("{error}\nPython rnsh log:\n{log}")));
            }
        };
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python rnsh initiator failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("python-rnsh-to-rust\n"),
            "Python rnsh did not preserve remote output: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        Ok(())
    })();

    let _ = listener.kill();
    let _ = listener.wait();
    result
}
