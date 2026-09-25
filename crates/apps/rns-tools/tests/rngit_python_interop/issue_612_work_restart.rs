#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_work_survives_process_restart_for_pinned_python_client() -> io::Result<()> {
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
    let port = free_port()?;
    let identity_seed = "rngit-python-restart-server";
    let config_dir = temp.path().join("python-client");
    fs::create_dir_all(&config_dir)?;
    write_python_config(&config_dir, port)?;
    let identity = config_dir.join("identity");
    let destination = rust_git_destination(&root, identity_seed)?;
    let mut server = spawn_rngit_server(&root, port, identity_seed)?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let created = run_python_work_client(
            &python_repo,
            &config_dir,
            &identity,
            &destination,
            "create",
            None,
        )?;
        if !created.status.success() {
            return Err(io::Error::other(format!(
                "Python work creator failed: {}\nstdout:\n{}\nstderr:\n{}",
                created.status,
                String::from_utf8_lossy(&created.stdout),
                String::from_utf8_lossy(&created.stderr)
            )));
        }
        let created_json: serde_json::Value =
            serde_json::from_slice(&created.stdout).map_err(|error| {
                io::Error::other(format!(
                    "Python work creator returned invalid JSON: {error}\nstdout:\n{}",
                    String::from_utf8_lossy(&created.stdout)
                ))
            })?;
        let work_id = created_json
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| io::Error::other("Python work creator omitted numeric ID"))?;
        if created_json.get("scope").and_then(serde_json::Value::as_str) != Some("active") {
            return Err(io::Error::other("Python work creator did not return active scope"));
        }
        server.kill()?;
        server.wait()?;
        server = spawn_rngit_server(&root, port, identity_seed)?;
        wait_for_port(port, &mut server)?;
        let verified = run_python_work_client(
            &python_repo,
            &config_dir,
            &identity,
            &destination,
            "verify",
            Some(work_id),
        )?;
        if !verified.status.success() {
            return Err(io::Error::other(format!(
                "Python work verifier failed: {}\nstdout:\n{}\nstderr:\n{}",
                verified.status,
                String::from_utf8_lossy(&verified.stdout),
                String::from_utf8_lossy(&verified.stderr)
            )));
        }
        let verified_json: serde_json::Value =
            serde_json::from_slice(&verified.stdout).map_err(|error| {
                io::Error::other(format!(
                    "Python work verifier returned invalid JSON: {error}\nstdout:\n{}",
                    String::from_utf8_lossy(&verified.stdout)
                ))
            })?;
        if verified_json.get("persisted") != Some(&serde_json::Value::Bool(true))
            || verified_json.get("content").and_then(serde_json::Value::as_str)
                != Some("Python restart work document body")
            || verified_json.get("title").and_then(serde_json::Value::as_str)
                != Some("Python restart work")
            || verified_json.get("comment_persisted") != Some(&serde_json::Value::Bool(true))
        {
            return Err(io::Error::other(format!(
                "work document did not survive restart: {verified_json}"
            )));
        }
        Ok(())
    })();

    let _ = server.kill();
    let _ = server.wait();
    result
}
