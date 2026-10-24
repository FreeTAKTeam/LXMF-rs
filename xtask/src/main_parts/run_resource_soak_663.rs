fn run_resource_soak_663(args: &[String]) -> Result<()> {
    let python = if cfg!(windows) { "python" } else { "python3" };
    let mut borrowed = vec!["tools/scripts/resource_soak_663.py"];
    borrowed.extend(args.iter().map(String::as_str));
    run(python, &borrowed)
}
