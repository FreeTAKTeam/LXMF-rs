use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
            return Err(io::Error::other(format!("peer exited before opening its port: {status}")));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "peer did not open its TCP port"))
}

fn write_python_config(dir: &Path, kind: &str, port: u16) -> io::Result<()> {
    let interface = match kind {
        "client" => format!(
            "[[TCP Client Interface]]\n\
             type = TCPClientInterface\n\
             enabled = yes\n\
             target_host = 127.0.0.1\n\
             target_port = {port}\n"
        ),
        "server" => format!(
            "[[TCP Server Interface]]\n\
             type = TCPServerInterface\n\
             enabled = yes\n\
             listen_ip = 127.0.0.1\n\
             listen_port = {port}\n"
        ),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown Python interface kind",
            ))
        }
    };
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
             {interface}"
        ),
    )
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

fn python_identity_output(
    python: &str,
    config_dir: &Path,
    identity: &Path,
    repo: &Path,
) -> io::Result<String> {
    let output = Command::new(python)
        .arg("-c")
        .arg(
            "import os, sys, RNS; identity = RNS.Identity.from_file(sys.argv[1]) if os.path.isfile(sys.argv[1]) else RNS.Identity(); identity.to_file(sys.argv[1]); RNS.Reticulum(configdir=sys.argv[2], loglevel=0); destination = RNS.Destination(identity, RNS.Destination.IN, RNS.Destination.SINGLE, 'rncp', 'receive'); print(destination.hash.hex())",
        )
        .arg(identity)
        .arg(config_dir)
        .env("PYTHONPATH", repo)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Python rncp identity query failed: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let destination = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !destination.is_empty() {
        return Ok(destination);
    }
    Err(io::Error::other(format!(
        "Python rncp identity query omitted destination\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )))
}

fn python_identity_hash(
    python: &str,
    config_dir: &Path,
    identity: &Path,
    repo: &Path,
) -> io::Result<String> {
    let output = Command::new(python)
        .arg("-c")
        .arg(
            "import os, sys, RNS; identity = RNS.Identity.from_file(sys.argv[1]) if os.path.isfile(sys.argv[1]) else RNS.Identity(); identity.to_file(sys.argv[1]); print(identity.hash.hex())",
        )
        .arg(identity)
        .arg(config_dir)
        .env("PYTHONPATH", repo)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Python rncp identity hash query failed: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let hash = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !hash.is_empty() {
        return Ok(hash);
    }
    Err(io::Error::other("Python rncp identity hash query omitted identity"))
}

fn rust_identity_hash(identity_seed: &str) -> io::Result<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .args(["--print-identity", "--identity-seed", identity_seed])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Rust rncp identity query failed: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Identity     : "))
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other("Rust rncp identity query omitted identity hash"))
}

fn run_python_send(
    python: &str,
    script: &Path,
    config_dir: &Path,
    identity: &Path,
    repo: &Path,
    source: &Path,
    destination: &str,
) -> io::Result<Output> {
    let output = Command::new(python)
        .arg(script)
        .arg(source)
        .arg(destination)
        .arg("--config")
        .arg(config_dir)
        .arg("-i")
        .arg(identity)
        .args(["-S", "-C", "-w", "30"])
        .env("PYTHONPATH", repo)
        .output()?;
    Ok(output)
}

fn run_rust_send(
    source: &Path,
    destination: &str,
    port: u16,
    identity_seed: &str,
) -> io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_rncp"))
        .arg(source)
        .arg(destination)
        .args([
            "--connect",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            identity_seed,
            "--silent",
            "--no-compress",
            "--timeout",
            "30",
        ])
        .output()
}

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rncp_exchanges_binary_files_with_pinned_python_in_both_directions() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let rust_listener_root = temp.path().join("rust-listener");
    let python_listener_root = temp.path().join("python-listener");
    let rust_source_root = temp.path().join("rust-source");
    let python_source_root = temp.path().join("python-source");
    for directory in
        [&rust_listener_root, &python_listener_root, &rust_source_root, &python_source_root]
    {
        fs::create_dir_all(directory)?;
    }
    let rust_payload = (0..16_384).map(|index| (index as u8).wrapping_mul(37)).collect::<Vec<_>>();
    let python_payload =
        (0..12_345).map(|index| (index as u8).wrapping_mul(19).wrapping_add(7)).collect::<Vec<_>>();
    let rust_source = rust_source_root.join("rust-to-python.bin");
    let python_source = python_source_root.join("python-to-rust.bin");
    fs::write(&rust_source, &rust_payload)?;
    fs::write(&python_source, &python_payload)?;

    let repo = python_repo();
    let python = python_bin();
    let script = repo.join("RNS/Utilities/rncp.py");
    if !script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rncp script not found: {}", script.display()),
        ));
    }

    let python_sender_config = temp.path().join("python-sender");
    fs::create_dir_all(&python_sender_config)?;
    let python_sender_identity = python_sender_config.join("identity");
    let python_sender_hash =
        python_identity_hash(&python, &python_sender_config, &python_sender_identity, &repo)?;

    let rust_listener_port = free_port()?;
    let rust_identity_seed = "rncp-python-interop-rust-listener";
    let mut rust_listener = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .args([
            "--listen",
            &format!("127.0.0.1:{rust_listener_port}"),
            "--allowed-identity",
            &python_sender_hash,
            "--save",
        ])
        .arg(&rust_listener_root)
        .args(["--identity-seed", rust_identity_seed, "--silent", "--timeout", "30"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(rust_listener_port, &mut rust_listener)?;
        let rust_destination = Command::new(env!("CARGO_BIN_EXE_rncp"))
            .args(["--print-identity", "--identity-seed", rust_identity_seed])
            .output()?;
        if !rust_destination.status.success() {
            return Err(io::Error::other("Rust rncp identity query failed"));
        }
        let rust_destination = String::from_utf8_lossy(&rust_destination.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("Rust rncp identity query omitted destination"))?
            .to_owned();

        write_python_config(&python_sender_config, "client", rust_listener_port)?;
        let python_output = run_python_send(
            &python,
            &script,
            &python_sender_config,
            &python_sender_identity,
            &repo,
            &python_source,
            &rust_destination,
        )?;
        if !python_output.status.success() {
            return Err(io::Error::other(format!(
                "Python rncp sender failed: {}\nstdout:\n{}\nstderr:\n{}",
                python_output.status,
                String::from_utf8_lossy(&python_output.stdout),
                String::from_utf8_lossy(&python_output.stderr)
            )));
        }
        assert_eq!(fs::read(rust_listener_root.join("python-to-rust.bin"))?, python_payload);
        Ok(())
    })();
    let _ = rust_listener.kill();
    let _ = rust_listener.wait();
    result?;

    let python_listener_port = free_port()?;
    let python_listener_config = temp.path().join("python-listener-config");
    fs::create_dir_all(&python_listener_config)?;
    write_python_config(&python_listener_config, "server", python_listener_port)?;
    let python_listener_identity = python_listener_config.join("identity");
    let python_destination =
        python_identity_output(&python, &python_listener_config, &python_listener_identity, &repo)?;
    let mut python_listener = Command::new(&python)
        .arg(&script)
        .arg("--listen")
        .arg("-a")
        .arg(rust_identity_hash("rncp-python-interop-rust-sender")?)
        .arg("--save")
        .arg(&python_listener_root)
        .arg("--config")
        .arg(&python_listener_config)
        .arg("-i")
        .arg(&python_listener_identity)
        .arg("-b")
        .arg("0")
        .env("PYTHONPATH", &repo)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(python_listener_port, &mut python_listener)?;
        let rust_output = run_rust_send(
            &rust_source,
            &python_destination,
            python_listener_port,
            "rncp-python-interop-rust-sender",
        )?;
        if !rust_output.status.success() {
            return Err(io::Error::other(format!(
                "Rust rncp sender failed: {}\nstdout:\n{}\nstderr:\n{}",
                rust_output.status,
                String::from_utf8_lossy(&rust_output.stdout),
                String::from_utf8_lossy(&rust_output.stderr)
            )));
        }
        assert_eq!(fs::read(python_listener_root.join("rust-to-python.bin"))?, rust_payload);
        Ok(())
    })();
    let _ = python_listener.kill();
    let _ = python_listener.wait();
    result
}
