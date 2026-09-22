use super::*;

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
