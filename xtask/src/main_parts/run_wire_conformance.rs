fn run_wire_conformance(output: Option<&Path>) -> Result<()> {
    let python = if cfg!(windows) { "python" } else { "python3" };
    let output = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("target/interop/python-rust-wire-conformance/report.json"));
    let output_string = output.to_string_lossy().into_owned();
    run(
        python,
        &[
            "tools/scripts/python_wire_conformance.py",
            "--output",
            output_string.as_str(),
        ],
    )?;
    run(
        "cargo",
        &[
            "test",
            "-p",
            "test-support",
            "--test",
            "wire_conformance",
            "--",
            "--nocapture",
        ],
    )
}
