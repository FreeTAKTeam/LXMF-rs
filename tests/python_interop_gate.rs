#[test]
fn python_interop_gate() {
    if std::env::var("LXMF_PYTHON_INTEROP").ok().as_deref() != Some("1") {
        eprintln!("skipping python interop gate; set LXMF_PYTHON_INTEROP=1 to enable");
        return;
    }

    let output = std::process::Command::new("python3")
        .args(["-c", "import LXMF, RNS; print('interop-ok')"])
        .output()
        .expect("python3 must be executable");

    assert!(
        output.status.success(),
        "python interop imports failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("interop-ok"));
}
