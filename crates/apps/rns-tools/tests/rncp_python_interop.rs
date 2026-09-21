use std::fs;
use std::io::{self, Write};
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

struct PythonRuntime<'a> {
    python: &'a str,
    script: &'a Path,
    repo: &'a Path,
}

fn run_python_send(
    runtime: &PythonRuntime<'_>,
    config_dir: &Path,
    identity: &Path,
    source: &Path,
    destination: &str,
    no_compress: bool,
) -> io::Result<Output> {
    let mut command = Command::new(runtime.python);
    command
        .arg(runtime.script)
        .arg(source)
        .arg(destination)
        .arg("--config")
        .arg(config_dir)
        .arg("-i")
        .arg(identity)
        .args(["-S", "-w", "30"]);
    if no_compress {
        command.arg("-C");
    }
    let output = command.env("PYTHONPATH", runtime.repo).output()?;
    Ok(output)
}

fn run_python_fetch(
    python: &str,
    config_dir: &Path,
    identity: &Path,
    repo: &Path,
    remote_file: &str,
    destination: &str,
    save_root: &Path,
) -> io::Result<Output> {
    let script = repo.join("RNS/Utilities/rncp.py");
    let mut child = Command::new(python)
        .arg(&script)
        .arg(remote_file)
        .arg(destination)
        .args(["--fetch", "--config"])
        .arg(config_dir)
        .arg("-i")
        .arg(identity)
        .arg("-s")
        .arg(save_root)
        .args(["-O", "-S", "-C", "-w", "30"])
        .env("PYTHONPATH", repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let output = child.wait_with_output()?;
            return Err(io::Error::other(format!(
                "Python rncp fetch timed out\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn run_rust_send(
    source: &Path,
    destination: &str,
    port: u16,
    identity_seed: &str,
    no_compress: bool,
) -> io::Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rncp"));
    command.arg(source).arg(destination).args([
        "--connect",
        &format!("127.0.0.1:{port}"),
        "--identity-seed",
        identity_seed,
        "--silent",
        "--timeout",
        "30",
    ]);
    if no_compress {
        command.arg("--no-compress");
    }
    command.output()
}

fn run_rust_fetch(
    remote_file: &Path,
    destination: &str,
    port: u16,
    identity_seed: &str,
    save_root: &Path,
    no_compress: bool,
) -> io::Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rncp"));
    command.arg(remote_file).arg(destination).args([
        "--fetch",
        "--connect",
        &format!("127.0.0.1:{port}"),
        "--identity-seed",
        identity_seed,
        "--save",
        save_root.to_string_lossy().as_ref(),
        "--silent",
        "--timeout",
        "30",
    ]);
    if no_compress {
        command.arg("--no-compress");
    }
    command.output()
}

fn spawn_python_listener(
    runtime: &PythonRuntime<'_>,
    config_dir: &Path,
    identity: &Path,
    root: &Path,
    allowed_identities: &[&str],
    no_compress: bool,
) -> io::Result<Child> {
    let mut command = Command::new(runtime.python);
    command.arg(runtime.script).arg("--listen");
    for allowed_identity in allowed_identities {
        command.arg("-a").arg(allowed_identity);
    }
    command
        .arg("--allow-fetch")
        .arg("--jail")
        .arg(root)
        .arg("--save")
        .arg(root)
        .arg("--config")
        .arg(config_dir)
        .arg("-i")
        .arg(identity)
        .arg("-b")
        .arg("0");
    if no_compress {
        command.arg("-C");
    }
    command.env("PYTHONPATH", runtime.repo).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()
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
    let rust_fetch_payload =
        (0..8_765).map(|index| (index as u8).wrapping_mul(29).wrapping_add(5)).collect::<Vec<_>>();
    let rust_source = rust_source_root.join("rust-to-python.bin");
    let python_source = python_source_root.join("python-to-rust.bin");
    let rust_fetch_source = rust_listener_root.join("rust-fetch-source.bin");
    fs::write(&rust_source, &rust_payload)?;
    fs::write(&python_source, &python_payload)?;
    fs::write(&rust_fetch_source, &rust_fetch_payload)?;
    fs::write(rust_listener_root.join("python-to-rust.bin"), b"stale receiver data")?;

    let repo = python_repo();
    let python = python_bin();
    let script = repo.join("RNS/Utilities/rncp.py");
    if !script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rncp script not found: {}", script.display()),
        ));
    }
    let python_runtime = PythonRuntime { python: &python, script: &script, repo: &repo };

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
        .args([
            "--identity-seed",
            rust_identity_seed,
            "--allow-fetch",
            "--overwrite",
            "--silent",
            "--timeout",
            "30",
        ])
        .current_dir(&rust_listener_root)
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
            &python_runtime,
            &python_sender_config,
            &python_sender_identity,
            &python_source,
            &rust_destination,
            true,
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
        assert!(!rust_listener_root.join("python-to-rust.bin.1").exists());

        let python_fetch_root = temp.path().join("python-fetch");
        fs::create_dir_all(&python_fetch_root)?;
        fs::write(python_fetch_root.join("rust-fetch-source.bin"), b"stale fetch data")?;
        let python_fetch_output = run_python_fetch(
            &python,
            &python_sender_config,
            &python_sender_identity,
            &repo,
            "rust-fetch-source.bin",
            &rust_destination,
            &python_fetch_root,
        )?;
        if !python_fetch_output.status.success() {
            return Err(io::Error::other(format!(
                "Python rncp fetch from Rust failed: {}\nstdout:\n{}\nstderr:\n{}",
                python_fetch_output.status,
                String::from_utf8_lossy(&python_fetch_output.stdout),
                String::from_utf8_lossy(&python_fetch_output.stderr)
            )));
        }
        assert_eq!(fs::read(python_fetch_root.join("rust-fetch-source.bin"))?, rust_fetch_payload);
        assert!(!python_fetch_root.join("rust-fetch-source.bin.1").exists());
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
    let python_fetch_source = python_listener_root.join("fetch-source.bin");
    let python_fetch_payload =
        (0..9_876).map(|index| (index as u8).wrapping_mul(23).wrapping_add(11)).collect::<Vec<_>>();
    fs::write(&python_fetch_source, &python_fetch_payload)?;
    let rust_sender_hash = rust_identity_hash("rncp-python-interop-rust-sender")?;
    let rust_fetch_identity_seed = "rncp-python-interop-rust-fetcher";
    let rust_fetch_hash = rust_identity_hash(rust_fetch_identity_seed)?;
    let mut python_listener = spawn_python_listener(
        &python_runtime,
        &python_listener_config,
        &python_listener_identity,
        &python_listener_root,
        &[&rust_sender_hash, &rust_fetch_hash],
        false,
    )?;
    let result = (|| {
        wait_for_port(python_listener_port, &mut python_listener)?;
        let rust_output = run_rust_send(
            &rust_source,
            &python_destination,
            python_listener_port,
            "rncp-python-interop-rust-sender",
            true,
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

        let rust_fetch_root = temp.path().join("rust-fetch");
        fs::create_dir_all(&rust_fetch_root)?;
        let fetched = run_rust_fetch(
            Path::new("fetch-source.bin"),
            &python_destination,
            python_listener_port,
            rust_fetch_identity_seed,
            &rust_fetch_root,
            true,
        )?;
        if !fetched.status.success() {
            return Err(io::Error::other(format!(
                "Rust rncp fetch from Python failed: {}\nstdout:\n{}\nstderr:\n{}",
                fetched.status,
                String::from_utf8_lossy(&fetched.stdout),
                String::from_utf8_lossy(&fetched.stderr)
            )));
        }
        assert_eq!(fs::read(rust_fetch_root.join("fetch-source.bin"))?, python_fetch_payload);

        let denied = run_rust_fetch(
            Path::new("fetch-source.bin"),
            &python_destination,
            python_listener_port,
            "rncp-python-interop-rust-denied",
            &rust_fetch_root,
            true,
        )?;
        if denied.status.success() {
            return Err(io::Error::other("unauthorised Python rncp fetch unexpectedly succeeded"));
        }
        Ok(())
    })();
    let _ = python_listener.kill();
    let _ = python_listener.wait();
    result
}

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rncp_mixed_runtime_compression_matrix_roundtrips_binary_files() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let repo = python_repo();
    let python = python_bin();
    let script = repo.join("RNS/Utilities/rncp.py");
    if !script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rncp script not found: {}", script.display()),
        ));
    }
    let python_runtime = PythonRuntime { python: &python, script: &script, repo: &repo };

    let compressible_payload = b"reticulum rncp mixed-runtime compression payload ".repeat(600);
    let already_compressed_source = b"already compressed source material ".repeat(600);
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    encoder.write_all(&already_compressed_source)?;
    let already_compressed_payload = encoder.finish()?;

    let rust_listener_root = temp.path().join("rust-listener");
    let python_sender_root = temp.path().join("python-sender-root");
    fs::create_dir_all(&rust_listener_root)?;
    fs::create_dir_all(&python_sender_root)?;
    let python_sender_config = temp.path().join("python-sender-config");
    fs::create_dir_all(&python_sender_config)?;
    let python_sender_identity = python_sender_config.join("identity");
    let python_sender_hash =
        python_identity_hash(&python, &python_sender_config, &python_sender_identity, &repo)?;
    let python_default_source = python_sender_root.join("python-default.bin");
    let python_no_compress_source = python_sender_root.join("python-no-compress.bz2");
    fs::write(&python_default_source, &compressible_payload)?;
    fs::write(&python_no_compress_source, &already_compressed_payload)?;

    let rust_listener_port = free_port()?;
    let rust_listener_seed = "rncp-python-compression-rust-listener";
    let mut rust_listener = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .args([
            "--listen",
            &format!("127.0.0.1:{rust_listener_port}"),
            "--allowed-identity",
            &python_sender_hash,
            "--save",
        ])
        .arg(&rust_listener_root)
        .args(["--identity-seed", rust_listener_seed, "--overwrite", "--silent", "--timeout", "30"])
        .current_dir(&rust_listener_root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(rust_listener_port, &mut rust_listener)?;
        let destination_output = Command::new(env!("CARGO_BIN_EXE_rncp"))
            .args(["--print-identity", "--identity-seed", rust_listener_seed])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("Rust rncp identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("Rust rncp identity query omitted destination"))?
            .to_owned();
        write_python_config(&python_sender_config, "client", rust_listener_port)?;

        for (source, no_compress) in
            [(&python_default_source, false), (&python_no_compress_source, true)]
        {
            let output = run_python_send(
                &python_runtime,
                &python_sender_config,
                &python_sender_identity,
                source,
                &destination,
                no_compress,
            )?;
            if !output.status.success() {
                return Err(io::Error::other(format!(
                    "Python rncp compression-matrix sender failed: {}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
        }
        assert_eq!(fs::read(rust_listener_root.join("python-default.bin"))?, compressible_payload);
        assert_eq!(
            fs::read(rust_listener_root.join("python-no-compress.bz2"))?,
            already_compressed_payload
        );
        Ok(())
    })();
    let _ = rust_listener.kill();
    let _ = rust_listener.wait();
    result?;

    let python_listener_root = temp.path().join("python-listener");
    fs::create_dir_all(&python_listener_root)?;
    let python_listener_source = python_listener_root.join("fetch-default.bin");
    fs::write(&python_listener_source, &compressible_payload)?;
    let python_listener_config = temp.path().join("python-listener-config");
    fs::create_dir_all(&python_listener_config)?;
    let python_listener_port = free_port()?;
    write_python_config(&python_listener_config, "server", python_listener_port)?;
    let python_listener_identity = python_listener_config.join("identity");
    let python_destination =
        python_identity_output(&python, &python_listener_config, &python_listener_identity, &repo)?;
    let rust_sender_seed = "rncp-python-compression-rust-sender";
    let rust_fetch_seed = "rncp-python-compression-rust-fetcher";
    let rust_sender_hash = rust_identity_hash(rust_sender_seed)?;
    let rust_fetch_hash = rust_identity_hash(rust_fetch_seed)?;
    let mut python_listener = spawn_python_listener(
        &python_runtime,
        &python_listener_config,
        &python_listener_identity,
        &python_listener_root,
        &[&rust_sender_hash, &rust_fetch_hash],
        false,
    )?;
    let result = (|| {
        wait_for_port(python_listener_port, &mut python_listener)?;
        for (source, no_compress, expected_name, expected) in [
            (
                python_default_source.as_path(),
                false,
                "python-default.bin",
                compressible_payload.as_slice(),
            ),
            (
                python_no_compress_source.as_path(),
                true,
                "python-no-compress.bz2",
                already_compressed_payload.as_slice(),
            ),
        ] {
            let output = run_rust_send(
                source,
                &python_destination,
                python_listener_port,
                rust_sender_seed,
                no_compress,
            )?;
            if !output.status.success() {
                return Err(io::Error::other(format!(
                    "Rust rncp compression-matrix sender failed: {}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            assert_eq!(fs::read(python_listener_root.join(expected_name))?, expected);
        }

        let rust_fetch_root = temp.path().join("rust-fetch-default");
        fs::create_dir_all(&rust_fetch_root)?;
        let fetched = run_rust_fetch(
            Path::new("fetch-default.bin"),
            &python_destination,
            python_listener_port,
            rust_fetch_seed,
            &rust_fetch_root,
            false,
        )?;
        if !fetched.status.success() {
            return Err(io::Error::other(format!(
                "Rust rncp default-compression fetch failed: {}\nstdout:\n{}\nstderr:\n{}",
                fetched.status,
                String::from_utf8_lossy(&fetched.stdout),
                String::from_utf8_lossy(&fetched.stderr)
            )));
        }
        assert_eq!(fs::read(rust_fetch_root.join("fetch-default.bin"))?, compressible_payload);
        Ok(())
    })();
    let _ = python_listener.kill();
    let _ = python_listener.wait();
    result?;

    let python_no_compress_config = temp.path().join("python-no-compress-config");
    fs::create_dir_all(&python_no_compress_config)?;
    let python_no_compress_port = free_port()?;
    write_python_config(&python_no_compress_config, "server", python_no_compress_port)?;
    let python_no_compress_identity = python_no_compress_config.join("identity");
    let python_no_compress_destination = python_identity_output(
        &python,
        &python_no_compress_config,
        &python_no_compress_identity,
        &repo,
    )?;
    let mut python_no_compress_listener = spawn_python_listener(
        &python_runtime,
        &python_no_compress_config,
        &python_no_compress_identity,
        &python_listener_root,
        &[&rust_fetch_hash],
        true,
    )?;
    let result = (|| {
        wait_for_port(python_no_compress_port, &mut python_no_compress_listener)?;
        let rust_fetch_root = temp.path().join("rust-fetch-no-compress");
        fs::create_dir_all(&rust_fetch_root)?;
        let fetched = run_rust_fetch(
            Path::new("fetch-default.bin"),
            &python_no_compress_destination,
            python_no_compress_port,
            rust_fetch_seed,
            &rust_fetch_root,
            false,
        )?;
        if !fetched.status.success() {
            return Err(io::Error::other(format!(
                "Rust rncp no-compression fetch failed: {}\nstdout:\n{}\nstderr:\n{}",
                fetched.status,
                String::from_utf8_lossy(&fetched.stdout),
                String::from_utf8_lossy(&fetched.stderr)
            )));
        }
        assert_eq!(fs::read(rust_fetch_root.join("fetch-default.bin"))?, compressible_payload);
        Ok(())
    })();
    let _ = python_no_compress_listener.kill();
    let _ = python_no_compress_listener.wait();
    result
}

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rncp_python_listener_restart_preserves_identity_and_transfer() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let repo = python_repo();
    let python = python_bin();
    let script = repo.join("RNS/Utilities/rncp.py");
    if !script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rncp script not found: {}", script.display()),
        ));
    }
    let python_runtime = PythonRuntime { python: &python, script: &script, repo: &repo };
    let listener_root = temp.path().join("python-listener");
    let source_root = temp.path().join("rust-source");
    let config_dir = temp.path().join("python-config");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&source_root)?;
    fs::create_dir_all(&config_dir)?;

    let port = free_port()?;
    write_python_config(&config_dir, "server", port)?;
    let identity = config_dir.join("identity");
    let destination = python_identity_output(&python, &config_dir, &identity, &repo)?;
    let rust_sender_seed = "rncp-python-restart-rust-sender";
    let rust_sender_hash = rust_identity_hash(rust_sender_seed)?;
    let mut listener = spawn_python_listener(
        &python_runtime,
        &config_dir,
        &identity,
        &listener_root,
        &[&rust_sender_hash],
        true,
    )?;
    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let first_payload = b"python listener before restart";
        let first_source = source_root.join("before-restart.bin");
        fs::write(&first_source, first_payload)?;
        let first = run_rust_send(&first_source, &destination, port, rust_sender_seed, true)?;
        if !first.status.success() {
            return Err(io::Error::other(format!(
                "Rust rncp send before Python restart failed: {}\nstdout:\n{}\nstderr:\n{}",
                first.status,
                String::from_utf8_lossy(&first.stdout),
                String::from_utf8_lossy(&first.stderr)
            )));
        }
        assert_eq!(fs::read(listener_root.join("before-restart.bin"))?, first_payload);

        listener.kill()?;
        listener.wait()?;
        let restarted_destination = python_identity_output(&python, &config_dir, &identity, &repo)?;
        assert_eq!(restarted_destination, destination);
        listener = spawn_python_listener(
            &python_runtime,
            &config_dir,
            &identity,
            &listener_root,
            &[&rust_sender_hash],
            true,
        )?;
        wait_for_port(port, &mut listener)?;

        let second_payload = b"python listener after restart";
        let second_source = source_root.join("after-restart.bin");
        fs::write(&second_source, second_payload)?;
        let second = run_rust_send(&second_source, &destination, port, rust_sender_seed, true)?;
        if !second.status.success() {
            return Err(io::Error::other(format!(
                "Rust rncp send after Python restart failed: {}\nstdout:\n{}\nstderr:\n{}",
                second.status,
                String::from_utf8_lossy(&second.stdout),
                String::from_utf8_lossy(&second.stderr)
            )));
        }
        assert_eq!(fs::read(listener_root.join("after-restart.bin"))?, second_payload);
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}
