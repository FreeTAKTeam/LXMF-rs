use std::fs;
use std::io;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

const UNKNOWN_DESTINATION: &str = "00112233445566778899aabbccddeeff";

fn python_repo() -> PathBuf {
    std::env::var_os("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".tmp/python-refs/Reticulum"))
}

fn tree_entries(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                pending.push(path.clone());
            }
            entries.push(path.strip_prefix(root).expect("entry is under root").to_path_buf());
        }
    }
    entries.sort();
    Ok(entries)
}

#[test]
#[ignore = "requires the frozen Python Reticulum checkout"]
fn rncp_unknown_destination_records_python_and_rust_cli_transcripts() -> io::Result<()> {
    let python_repo = python_repo();
    let python_script = python_repo.join("RNS/Utilities/rncp.py");
    if !python_script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("frozen Python rncp script not found: {}", python_script.display()),
        ));
    }

    let temp = tempfile::tempdir()?;
    let source = temp.path().join("payload.bin");
    fs::write(&source, b"unknown destination fixture")?;
    let python_home = temp.path().join("python-home");
    let python_config = temp.path().join("python-config");
    let rust_home = temp.path().join("rust-home");
    let rust_config = temp.path().join("rust-config");
    fs::create_dir_all(&python_home)?;
    fs::create_dir_all(&python_config)?;
    fs::create_dir_all(&rust_home)?;
    fs::create_dir_all(&rust_config)?;
    fs::write(
        python_config.join("config"),
        "[reticulum]\n  enable_transport = No\n  share_instance = No\n\n[logging]\n  loglevel = 0\n\n[interfaces]\n",
    )?;

    let python =
        Command::new(std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into()))
            .arg(&python_script)
            .args(["--config"])
            .arg(&python_config)
            .args(["-w", "1", "-S"])
            .arg(&source)
            .arg(UNKNOWN_DESTINATION)
            .current_dir(&python_home)
            .env("PYTHONPATH", &python_repo)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("HOME", &python_home)
            .env("XDG_CONFIG_HOME", &python_config)
            .output()?;

    // Keep the Rust production TCP interface local and inert: the TCP
    // handshake completes, but no Reticulum peer announces the destination.
    let sink = TcpListener::bind("127.0.0.1:0")?;
    let sink_address = sink.local_addr()?;
    let rust = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .arg(&source)
        .arg(UNKNOWN_DESTINATION)
        .args(["--connect", &sink_address.to_string(), "--timeout", "1"])
        .current_dir(&rust_home)
        .env("HOME", &rust_home)
        .env("XDG_CONFIG_HOME", &rust_config)
        .output()?;
    drop(sink);

    assert_eq!(python.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&python.stdout),
        format!("\u{1b}[2K\rPath to <{UNKNOWN_DESTINATION}> requested\n")
    );
    assert!(python.stderr.is_empty(), "Python stderr: {}", String::from_utf8_lossy(&python.stderr));
    assert_eq!(rust.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&rust.stdout),
        format!("Path to {UNKNOWN_DESTINATION} requested\n")
    );
    assert_eq!(String::from_utf8_lossy(&rust.stderr), "rncp: path discovery timed out\n");

    assert_eq!(fs::read(&source)?, b"unknown destination fixture");
    let python_home_entries = tree_entries(&python_home)?;
    let python_config_entries = tree_entries(&python_config)?;
    let rust_home_entries = tree_entries(&rust_home)?;
    let rust_config_entries = tree_entries(&rust_config)?;
    assert!(python_home_entries.is_empty(), "Python home side effects: {python_home_entries:?}");
    assert!(
        python_config_entries.contains(&PathBuf::from("storage/identities/rncp")),
        "Python Reticulum state was not isolated under its config root: {python_config_entries:?}"
    );
    assert!(
        !python_config_entries
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == "payload.bin")),
        "Python created a transferred file: {python_config_entries:?}"
    );
    assert!(rust_home_entries.is_empty(), "Rust home side effects: {rust_home_entries:?}");
    assert!(rust_config_entries.is_empty(), "Rust config side effects: {rust_config_entries:?}");

    Ok(())
}
