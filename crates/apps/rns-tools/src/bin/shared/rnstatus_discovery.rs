fn write_discovered_status(
    output: &mut dyn Write,
    rows: &Value,
    json_output: bool,
    show_stale: bool,
    show_unknown: bool,
) -> io::Result<()> {
    let rows = rows.as_array().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "discovered interfaces must be an array")
    })?;
    if json_output {
        return writeln!(output, "{}", serde_json::to_string_pretty(rows)?);
    }
    let visible = rows
        .iter()
        .filter(|row| {
            let stale = row.get("status").and_then(Value::as_str) == Some("stale");
            let known_implementation = ["impl_name", "version"].iter().all(|field| {
                row.get(*field).and_then(Value::as_str).is_some_and(|value| !value.is_empty())
            });
            (!stale || show_stale) && (known_implementation || show_unknown)
        })
        .collect::<Vec<_>>();
    writeln!(output, "{:<25} {:<20} {:<10} {:<20} Value", "Name", "Type", "Status", "Running")?;
    for row in visible {
        let name = row.get("name").and_then(Value::as_str).unwrap_or("-");
        let kind = row.get("type").and_then(Value::as_str).unwrap_or("-");
        let status = row.get("status").and_then(Value::as_str).unwrap_or("-");
        let implementation = row.get("impl_name").and_then(Value::as_str).unwrap_or("Unknown");
        let version = row.get("version").and_then(Value::as_str).unwrap_or("");
        let running = if version.is_empty() {
            implementation.to_string()
        } else {
            format!("{implementation} {version}")
        };
        let value = row.get("value").and_then(Value::as_u64).unwrap_or(0);
        writeln!(output, "{name:<25} {kind:<20} {status:<10} {running:<20} {value}")?;
    }
    Ok(())
}
