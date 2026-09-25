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
fn rnid_sign_emits_python_validated_binary_rsg_and_rejects_tampering() -> io::Result<()> {
    let python_repo = python_repo();
    let python_script = python_repo.join("RNS/Utilities/rnid.py");
    if !python_script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("frozen Python rnid script not found: {}", python_script.display()),
        ));
    }

    let temp = tempfile::tempdir()?;
    let identity_path = temp.path().join("signer.rid");
    let first_input = temp.path().join("binary-payload-one.bin");
    let second_input = temp.path().join("binary-payload-two.bin");
    let first_signature = PathBuf::from(format!("{}.rsg", first_input.display()));
    let second_signature = PathBuf::from(format!("{}.rsg", second_input.display()));
    fs::write(&first_input, [0, 1, 127, 128, 254, 255, 0])?;
    fs::write(&second_input, [255, 0, 128, 2, 0, 254])?;

    let generated = Command::new(env!("CARGO_BIN_EXE_rnid"))
        .args(["generate", "--output"])
        .arg(&identity_path)
        .output()?;
    assert!(
        generated.status.success(),
        "rnid generate failed: {}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let existing_signature = b"preserve this pre-existing signature";
    fs::write(&second_signature, existing_signature)?;
    let python_no_force =
        Command::new(std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into()))
            .arg(&python_script)
            .arg("--identity")
            .arg(&identity_path)
            .arg("--sign")
            .arg(&first_input)
            .arg(&second_input)
            .current_dir(temp.path())
            .env("PYTHONPATH", &python_repo)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()?;
    assert_eq!(python_no_force.status.code(), Some(11));
    assert!(
        python_no_force.stderr.is_empty(),
        "pinned Python wrote no-overwrite diagnostic to stderr"
    );
    let python_stdout = String::from_utf8(python_no_force.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let expected_diagnostic = format!(
        "The signature file \"{}\" already exists, not overwriting\n",
        second_signature.display()
    );
    assert!(
        python_stdout.ends_with(&expected_diagnostic),
        "unexpected pinned-Python stdout: {python_stdout:?}"
    );
    assert!(fs::read(&first_signature)?.len() > 64, "pinned Python did not sign first input");
    assert_eq!(
        fs::read(&second_signature)?,
        existing_signature,
        "pinned Python changed existing signature"
    );

    // Reset the first output so Rust encounters the existing second output,
    // matching the same two-path order as the reference process above.
    fs::remove_file(&first_signature)?;
    let no_force = Command::new(env!("CARGO_BIN_EXE_rnid"))
        .arg("--identity")
        .arg(&identity_path)
        .arg("--sign")
        .arg(&first_input)
        .arg(&second_input)
        .output()?;
    assert_eq!(no_force.status.code(), Some(11));
    assert!(no_force.stderr.is_empty(), "status-11 diagnostic went to stderr");
    let no_force_stdout = String::from_utf8(no_force.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    assert!(
        no_force_stdout.starts_with(&format!("Signed file {} with ", first_input.display())),
        "first path did not complete before the second path failed: {no_force_stdout:?}"
    );
    assert!(
        no_force_stdout.ends_with(&expected_diagnostic),
        "unexpected no-overwrite stdout: {no_force_stdout:?}"
    );
    assert!(fs::read(&first_signature)?.len() > 64, "first input was not signed");
    assert_eq!(
        fs::read(&second_signature)?,
        existing_signature,
        "no-force changed existing signature"
    );

    let forced_overwrite = Command::new(env!("CARGO_BIN_EXE_rnid"))
        .arg("-i")
        .arg(&identity_path)
        .arg("-s")
        .arg(&first_input)
        .arg(&second_input)
        .arg("--force")
        .output()?;
    assert!(
        forced_overwrite.status.success(),
        "rnid --force failed: {}",
        String::from_utf8_lossy(&forced_overwrite.stderr)
    );
    let first_signature_bytes = fs::read(&first_signature)?;
    let second_signature_bytes = fs::read(&second_signature)?;
    assert_ne!(
        second_signature_bytes, existing_signature,
        "--force did not replace signature output"
    );
    assert!(first_signature_bytes.len() > 64, "first signature missing after forced multi-sign");
    assert!(second_signature_bytes.len() > 64, "second signature missing after forced multi-sign");

    let validate = |input: &PathBuf| {
        Command::new(std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into()))
            .arg(&python_script)
            .arg("--identity")
            .arg(&identity_path)
            .arg("--validate")
            .arg(input)
            .current_dir(temp.path())
            .env("PYTHONPATH", &python_repo)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
    };
    for input in [&first_input, &second_input] {
        let valid = validate(input)?;
        assert_eq!(
            valid.status.code(),
            Some(0),
            "pinned Python rejected RSG for {}: {}",
            input.display(),
            String::from_utf8_lossy(&valid.stdout)
        );
        assert!(String::from_utf8_lossy(&valid.stdout).contains("Signature is valid"));
        assert!(
            valid.stderr.is_empty(),
            "Python stderr: {}",
            String::from_utf8_lossy(&valid.stderr)
        );
    }

    fs::write(&first_input, [0, 1, 127, 128, 254, 254, 0])?;
    let invalid = validate(&first_input)?;
    assert_eq!(
        invalid.status.code(),
        Some(10),
        "tampered payload was not rejected: {}",
        String::from_utf8_lossy(&invalid.stdout)
    );
    assert!(String::from_utf8_lossy(&invalid.stdout).contains("Invalid signature"));

    Ok(())
}
