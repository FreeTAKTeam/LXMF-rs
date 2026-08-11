fn run_independent_interop(
    peer: IndependentInteropPeer,
    level: IndependentInteropLevel,
    output: Option<&Path>,
    keep: bool,
) -> Result<()> {
    let python = if cfg!(windows) { "python" } else { "python3" };
    let mut args = vec![
        "tools/scripts/independent_interop.py".to_string(),
        "--peer".to_string(),
        match peer {
            IndependentInteropPeer::RnsRs => "rns-rs",
            IndependentInteropPeer::ReticulumGo => "reticulum-go",
        }
        .to_string(),
        "--level".to_string(),
        match level {
            IndependentInteropLevel::Pr => "pr",
            IndependentInteropLevel::Nightly => "nightly",
            IndependentInteropLevel::Release => "release",
        }
        .to_string(),
    ];
    if let Some(output) = output {
        args.push("--output".to_string());
        args.push(output.to_string_lossy().into_owned());
    }
    if keep {
        args.push("--keep".to_string());
    }
    let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
    run(python, &borrowed)
}
