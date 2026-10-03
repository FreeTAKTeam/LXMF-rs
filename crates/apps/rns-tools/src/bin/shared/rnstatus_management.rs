fn management_action(cli: &Cli) -> Option<(&'static str, &str)> {
    cli.attach
        .as_deref()
        .map(|name| ("attach", name))
        .or_else(|| cli.detach.as_deref().map(|name| ("detach", name)))
        .or_else(|| cli.reload.as_deref().map(|name| ("reload", name)))
}

fn run_management(
    cli: &Cli,
    output: &mut dyn Write,
    operation: &str,
    name: &str,
) -> io::Result<()> {
    let response = rpc_call_with_params(
        cli,
        1,
        "manage_interface",
        Some(json!({ "operation": operation, "name": name })),
    )?;
    let result = ensure_rpc_ok(response, "manage_interface")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing management result"))?;
    if result.get("complete").and_then(Value::as_bool) != Some(true) {
        return Err(io::Error::other("interface management did not complete"));
    }
    if cli.json {
        writeln!(output, "{}", serde_json::to_string_pretty(&result)?)
    } else {
        writeln!(output, "{operation} {name}: {}", result["state"].as_str().unwrap_or("complete"))
    }
}
