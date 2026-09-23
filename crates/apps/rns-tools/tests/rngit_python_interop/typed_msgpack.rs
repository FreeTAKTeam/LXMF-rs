use super::*;

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_work_storage_preserves_msgpack_binary_and_integer_types_both_directions() -> io::Result<()>
{
    let _guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("interop lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let work_dir = root.join("group/repo.work/active/73");
    fs::create_dir_all(&work_dir)?;
    let map = |entries: Vec<(&str, rmpv::Value)>| {
        rmpv::Value::Map(
            entries
                .into_iter()
                .map(|(key, value)| (rmpv::Value::String(key.into()), value))
                .collect(),
        )
    };
    let record = map(vec![
        ("content", rmpv::Value::String("typed fixture body".into())),
        (
            "meta",
            map(vec![
                ("format", rmpv::Value::String("markdown".into())),
                ("title", rmpv::Value::String("Typed fixture".into())),
                ("created", rmpv::Value::from(1_700_000_123_u64)),
                ("edited", rmpv::Value::from(1_700_000_456_u64)),
                ("author", rmpv::Value::Binary((0..16).collect())),
                ("identity", rmpv::Value::Binary((0..64).collect())),
                ("signature", rmpv::Value::Binary((0..64).rev().collect())),
            ]),
        ),
    ]);
    let mut bytes = Vec::new();
    rmpv::encode::write_value(&mut bytes, &record)
        .map_err(|error| io::Error::other(error.to_string()))?;
    fs::write(work_dir.join("root"), bytes)?;

    let port = free_port()?;
    let seed = "rngit-python-typed-wire-server";
    let config = temp.path().join("python-client");
    fs::create_dir_all(&config)?;
    write_python_config(&config, port)?;
    let identity = config.join("identity");
    let destination = rust_git_destination(&root, seed)?;
    let mut server = spawn_rngit_server(&root, port, seed)?;
    let result = (|| {
        wait_for_port(port, &mut server)?;
        let output = run_python_work_client(
            &python_repo(),
            &config,
            &identity,
            &destination,
            "verify_typed",
            Some(73),
        )?;
        if !output.status.success() || !String::from_utf8_lossy(&output.stdout).contains("true") {
            return Err(io::Error::other(format!(
                "Python typed-value verifier failed: {}\n{}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            )));
        }
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn python_work_binary_values_and_integer_timestamps_survive_rust_storage_restart() -> io::Result<()>
{
    let _guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("interop lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let python_repo = python_repo();
    let port = free_port()?;
    let seed = "rngit-python-restart-server";
    let config = temp.path().join("python-client");
    fs::create_dir_all(&config)?;
    write_python_config(&config, port)?;
    let identity = config.join("identity");
    let destination = rust_git_destination(&root, seed)?;
    let mut server = spawn_rngit_server(&root, port, seed)?;
    let result = (|| {
        wait_for_port(port, &mut server)?;
        let created =
            run_python_work_client(&python_repo, &config, &identity, &destination, "create", None)?;
        if !created.status.success() {
            return Err(io::Error::other(String::from_utf8_lossy(&created.stderr).into_owned()));
        }
        let json: serde_json::Value = serde_json::from_slice(&created.stdout)
            .map_err(|error| io::Error::other(error.to_string()))?;
        if json.get("scope").and_then(serde_json::Value::as_str) != Some("active") {
            return Err(io::Error::other("Python creator returned the wrong scope"));
        }
        let id = json
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| io::Error::other("Python creator omitted numeric ID"))?;
        let path = root.join("group/repo.work/active").join(id.to_string()).join("root");
        let bytes = fs::read(path)?;
        let record = rmpv::decode::read_value(&mut io::Cursor::new(bytes))
            .map_err(|error| io::Error::other(error.to_string()))?;
        let metadata = record
            .as_map()
            .and_then(|map| map.iter().find(|(key, _)| key.as_str() == Some("meta")))
            .and_then(|(_, value)| value.as_map())
            .ok_or_else(|| io::Error::other("stored metadata missing"))?;
        for (key, json_key) in
            [("author", "author_hex"), ("identity", "identity_hex"), ("signature", "signature_hex")]
        {
            let actual = metadata
                .iter()
                .find(|(name, _)| name.as_str() == Some(key))
                .and_then(|(_, value)| value.as_slice())
                .ok_or_else(|| io::Error::other(format!("{key} is not binary")))?;
            let expected = json
                .get(json_key)
                .and_then(serde_json::Value::as_str)
                .and_then(|value| hex::decode(value).ok())
                .ok_or_else(|| io::Error::other(format!("{json_key} missing")))?;
            if actual != expected {
                return Err(io::Error::other(format!("{key} bytes changed")));
            }
        }
        for key in ["created", "edited"] {
            if metadata
                .iter()
                .find(|(name, _)| name.as_str() == Some(key))
                .and_then(|(_, value)| value.as_u64())
                .is_none()
            {
                return Err(io::Error::other(format!("{key} timestamp is not integer")));
            }
        }
        server.kill()?;
        server.wait()?;
        server = spawn_rngit_server(&root, port, seed)?;
        wait_for_port(port, &mut server)?;
        let verified = run_python_work_client(
            &python_repo,
            &config,
            &identity,
            &destination,
            "verify",
            Some(id),
        )?;
        if !verified.status.success() {
            return Err(io::Error::other(String::from_utf8_lossy(&verified.stderr).into_owned()));
        }
        let verified: serde_json::Value = serde_json::from_slice(&verified.stdout)
            .map_err(|error| io::Error::other(error.to_string()))?;
        if verified.get("persisted") != Some(&serde_json::Value::Bool(true))
            || verified.get("content").and_then(serde_json::Value::as_str)
                != Some("Python restart work document body")
            || verified.get("title").and_then(serde_json::Value::as_str)
                != Some("Python restart work")
            || verified.get("comment_persisted") != Some(&serde_json::Value::Bool(true))
        {
            return Err(io::Error::other(format!(
                "work document did not survive restart: {verified}"
            )));
        }
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}
