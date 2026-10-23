#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rncp_exchanges_binary_files_with_pinned_python_in_both_directions() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let repo = python_repo();
    match std::env::var("RNS_REQUEST_RECEIPT_TRACE") {
        Ok(value) if value == "1.5.5" => {
            assert_python_revision(&repo, "7f2b3b9b524c9386316379af1313b43a5e4f7a5d")?
        }
        Err(std::env::VarError::NotPresent) => assert_pinned_python_revision(&repo)?,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported request trace reference",
            ))
        }
    }
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

    let python = python_bin();
    let script = repo.join("RNS/Utilities/rncp.py");
    if !script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rncp script not found: {}", script.display()),
        ));
    }
    let python_runtime =
        PythonRuntime { python: &python, script: &script, repo: &repo, resource_observation: None };

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
        assert!(
            String::from_utf8_lossy(&python_fetch_output.stdout).contains("Transfer complete"),
            "Python fetch callback did not emit completion status: {}",
            String::from_utf8_lossy(&python_fetch_output.stdout)
        );
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
        fs::write(rust_fetch_root.join("fetch-source.bin"), b"stale Rust fetch data")?;
        let fetched = run_rust_fetch(
            Path::new("fetch-source.bin"),
            &python_destination,
            python_listener_port,
            rust_fetch_identity_seed,
            &rust_fetch_root,
            true,
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
        assert!(
            !rust_fetch_root.join("fetch-source.bin.1").exists(),
            "Rust fetch created a collision suffix despite --overwrite"
        );

        let denied = run_rust_fetch(
            Path::new("fetch-source.bin"),
            &python_destination,
            python_listener_port,
            "rncp-python-interop-rust-denied",
            &rust_fetch_root,
            true,
            false,
        )?;
        if denied.status.success() {
            return Err(io::Error::other("unauthorised Python rncp fetch unexpectedly succeeded"));
        }
        assert_eq!(fs::read(rust_fetch_root.join("fetch-source.bin"))?, python_fetch_payload);
        assert!(
            !rust_fetch_root.join("fetch-source.bin.1").exists(),
            "denied fetch left an unexpected collision file"
        );
        Ok(())
    })();
    let _ = python_listener.kill();
    let _ = python_listener.wait();
    result
}
