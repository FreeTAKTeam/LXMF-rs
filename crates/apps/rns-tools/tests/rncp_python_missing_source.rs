use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

fn python_repo() -> PathBuf {
    std::env::var_os("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".tmp/python-refs/Reticulum"))
}

fn assert_empty_dir(path: &Path) -> io::Result<()> {
    assert_eq!(fs::read_dir(path)?.count(), 0, "unexpected file side effect in {}", path.display());
    Ok(())
}

#[test]
#[ignore = "requires the frozen Python Reticulum checkout"]
fn rncp_missing_source_records_python_and_rust_cli_transcripts() -> io::Result<()> {
    let python_repo = python_repo();
    let python_script = python_repo.join("RNS/Utilities/rncp.py");
    if !python_script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("frozen Python rncp script not found: {}", python_script.display()),
        ));
    }

    let temp = tempfile::tempdir()?;
    let missing_source = temp.path().join("missing-payload.bin");
    let python_home = temp.path().join("python-home");
    let rust_home = temp.path().join("rust-home");
    let python_config = temp.path().join("python-config");
    fs::create_dir_all(&python_home)?;
    fs::create_dir_all(&rust_home)?;
    fs::create_dir_all(&python_config)?;
    let destination_hash = "00112233445566778899aabbccddeeff";

    let python =
        Command::new(std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into()))
            .arg(&python_script)
            .arg(&missing_source)
            .arg(destination_hash)
            .current_dir(&python_home)
            .env("PYTHONPATH", &python_repo)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("HOME", &python_home)
            .env("XDG_CONFIG_HOME", &python_config)
            .output()?;
    let rust = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .arg(&missing_source)
        .arg(destination_hash)
        .current_dir(&rust_home)
        .env("HOME", &rust_home)
        .env("XDG_CONFIG_HOME", &rust_home)
        .output()?;

    assert_eq!(python.status.code(), Some(1));
    assert_eq!(python.stdout, b"File not found\n");
    assert!(python.stderr.is_empty(), "Python stderr: {}", String::from_utf8_lossy(&python.stderr));
    assert_eq!(rust.status.code(), Some(1));
    assert!(rust.stdout.is_empty(), "Rust stdout: {}", String::from_utf8_lossy(&rust.stdout));
    assert_eq!(
        String::from_utf8_lossy(&rust.stderr),
        "rncp: No such file or directory (os error 2)\n"
    );
    assert!(!missing_source.exists());
    assert_empty_dir(&python_home)?;
    assert_empty_dir(&python_config)?;
    assert_empty_dir(&rust_home)?;

    Ok(())
}
