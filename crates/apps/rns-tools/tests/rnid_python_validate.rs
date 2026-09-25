use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

fn python_repo() -> PathBuf {
    std::env::var_os("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".tmp/python-refs/Reticulum"))
}

#[test]
#[ignore = "requires the frozen Python Reticulum checkout"]
fn rnid_validates_pinned_python_signed_files_and_rejects_tampering() -> io::Result<()> {
    let python_repo = python_repo();
    let python_script = python_repo.join("RNS/Utilities/rnid.py");
    if !python_script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("frozen Python rnid script not found: {}", python_script.display()),
        ));
    }

    let temp = tempfile::tempdir()?;
    let identity = temp.path().join("signer.rid");
    let payload = temp.path().join("binary-payload.bin");
    let signature = PathBuf::from(format!("{}.rsg", payload.display()));
    let config = temp.path().join("python-config");
    fs::write(&payload, [0, 1, 127, 128, 254, 255, 0])?;

    let generated = Command::new(env!("CARGO_BIN_EXE_rnid"))
        .args(["generate", "--output"])
        .arg(&identity)
        .output()?;
    assert!(
        generated.status.success(),
        "rnid generate failed: {}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into());
    let signed = Command::new(&python)
        .arg(&python_script)
        .arg("--config")
        .arg(&config)
        .arg("--identity")
        .arg(&identity)
        .arg("--sign")
        .arg(&payload)
        .current_dir(temp.path())
        .env("PYTHONPATH", &python_repo)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()?;
    assert_eq!(signed.status.code(), Some(0), "Python signing failed");
    assert!(fs::metadata(&signature)?.len() > 64, "Python signature envelope is missing");

    let valid_rust =
        Command::new(env!("CARGO_BIN_EXE_rnid")).arg("--validate").arg(&payload).output()?;
    assert_eq!(
        valid_rust.status.code(),
        Some(0),
        "Rust rejected Python signature: {}",
        String::from_utf8_lossy(&valid_rust.stderr)
    );

    let valid_python = Command::new(&python)
        .arg(&python_script)
        .arg("--config")
        .arg(&config)
        .arg("--validate")
        .arg(&payload)
        .current_dir(temp.path())
        .env("PYTHONPATH", &python_repo)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()?;
    assert_eq!(valid_python.status.code(), Some(0), "Python rejected its valid signature");

    fs::write(&payload, [0, 1, 127, 128, 254, 254, 0])?;
    let invalid_rust =
        Command::new(env!("CARGO_BIN_EXE_rnid")).arg("--validate").arg(&signature).output()?;
    assert_eq!(invalid_rust.status.code(), Some(10), "Rust accepted changed payload");

    let invalid_python = Command::new(&python)
        .arg(&python_script)
        .arg("--config")
        .arg(&config)
        .arg("--validate")
        .arg(&signature)
        .current_dir(temp.path())
        .env("PYTHONPATH", &python_repo)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()?;
    assert_eq!(invalid_python.status.code(), Some(10), "Python accepted changed payload");

    Ok(())
}
