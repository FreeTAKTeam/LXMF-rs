fn run_go_compile_check(output_dir: &Path) -> Result<String> {
    if command_exists("go").is_none() {
        return Ok(format!("{COMPILER_CHECK_SKIP_PREFIX} go command not available"));
    }
    if !output_dir.exists() {
        return Ok(format!("{COMPILER_CHECK_SKIP_PREFIX} missing generated go output"));
    }

    let temp_parent = output_dir.parent().context("generated go output has no parent directory")?;
    let temp_dir = TempDir::new(temp_parent)?;
    let sandbox = temp_dir.path.join("go-compile-check");
    fs::create_dir_all(&sandbox).with_context(|| format!("create {}", sandbox.display()))?;
    copy_dir_recursively(output_dir, &sandbox)?;
    let go_mod = sandbox.join("go.mod");

    if !go_mod.exists() {
        let status = Command::new("go")
            .current_dir(&sandbox)
            .args(["mod", "init", "example.com/lxmfclient"])
            .status()
            .with_context(|| format!("spawn go mod init in {}", sandbox.display()))?;
        if !status.success() {
            return Ok(format!("FAIL: go mod init failed for {}", output_dir.display()));
        }
    }

    let status = Command::new("go")
        .current_dir(&sandbox)
        .args(["mod", "tidy"])
        .status()
        .with_context(|| format!("spawn go mod tidy in {}", sandbox.display()))?;
    if !status.success() {
        return Ok(format!("FAIL: go mod tidy failed for {}", output_dir.display()));
    }

    let status = Command::new("go")
        .current_dir(&sandbox)
        .args(["test", "./..."])
        .status()
        .with_context(|| format!("spawn go test in {}", sandbox.display()))?;
    if !status.success() {
        return Ok(format!("FAIL: go test failed for {}", output_dir.display()));
    }

    Ok(COMPILER_CHECK_PASS.to_string())
}

fn run_python_compile_check(output_dir: &Path) -> Result<String> {
    let python = if command_exists("python3").is_some() {
        "python3"
    } else if command_exists("python").is_some() {
        "python"
    } else {
        return Ok(format!("{COMPILER_CHECK_SKIP_PREFIX} python command not available"));
    };

    if !output_dir.exists() {
        return Ok(format!("{COMPILER_CHECK_SKIP_PREFIX} missing generated python output"));
    }
    let mut files = Vec::new();
    for path in collect_files_recursive(output_dir)? {
        let rel = path
            .strip_prefix(output_dir)
            .with_context(|| format!("compute relative python file path for {}", path.display()))?;
        if rel.extension().and_then(|value| value.to_str()) == Some("py") {
            files.push(rel.to_string_lossy().to_string());
        }
    }
    if files.is_empty() {
        return Ok(format!("{COMPILER_CHECK_SKIP_PREFIX} no python files"));
    }

    files.sort_unstable();

    let mut args = vec![
        "-c",
        r#"import sys

for path in sys.argv[1:]:
    try:
        with open(path, "rb") as handle:
            source = handle.read()
        compile(source, path, "exec")
    except (OSError, SyntaxError) as error:
        print(f"{path}: {error}")
        raise SystemExit(1)
"#,
    ];
    args.extend(files.iter().map(String::as_str));

    let status = Command::new(python)
        .current_dir(output_dir)
        .args(args)
        .status()
        .with_context(|| format!("spawn {python}"))?;
    if !status.success() {
        return Ok(format!("FAIL: python compile failed for {}", output_dir.display()));
    }

    Ok(COMPILER_CHECK_PASS.to_string())
}

fn run_typescript_compile_skip() -> String {
    format!("{COMPILER_CHECK_SKIP_PREFIX} tsc check not configured")
}

fn prepare_generator_openapi_spec(
    spec_path: &Path,
    openapi_version: &str,
    converted_path: &Path,
) -> Result<(PathBuf, String)> {
    if !openapi_version.starts_with("3.1") {
        return Ok((spec_path.to_path_buf(), openapi_version.to_string()));
    }

    let raw =
        fs::read_to_string(spec_path).with_context(|| format!("read {}", spec_path.display()))?;
    let spec = serde_json::from_str::<Value>(&raw)
        .with_context(|| format!("parse {}", spec_path.display()))?;
    let converted = convert_openapi_spec_for_generator(&spec)?;

    if let Some(parent) = converted_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(converted_path, serde_json::to_vec_pretty(&converted)?)
        .with_context(|| format!("write {}", converted_path.display()))?;

    Ok((converted_path.to_path_buf(), "3.0.3".to_string()))
}

fn convert_openapi_spec_for_generator(spec: &Value) -> Result<Value> {
    match spec {
        Value::Object(source) => {
            let mut out = Map::new();
            for (key, value) in source {
                match key.as_str() {
                    "openapi" => {
                        out.insert(key.clone(), json!("3.0.3"));
                    }
                    "$schema" | "$id" | "$defs" => {}
                    _ => {
                        out.insert(key.clone(), transform_schema_node_for_generator(value)?);
                    }
                }
            }
            Ok(Value::Object(out))
        }
        _ => Ok(spec.clone()),
    }
}

fn transform_schema_node_for_generator(value: &Value) -> Result<Value> {
    match value {
        Value::Array(values) => {
            let normalized = values
                .iter()
                .map(transform_schema_node_for_generator)
                .collect::<Result<Vec<_>>>()?;
            Ok(Value::Array(normalized))
        }
        Value::Object(map) => {
            let mut out = Map::new();

            let type_array = if let Some(types) = map.get("type").and_then(Value::as_array) {
                type_array_to_generator_type(types)?
            } else {
                None
            };

            for (key, node) in map {
                if key == "type" {
                    if let Some(TypeArrayConversion::Single { base_type, nullable }) = &type_array {
                        out.insert("type".to_string(), Value::String(base_type.to_string()));
                        if *nullable {
                            out.insert("nullable".to_string(), Value::Bool(true));
                        }
                    } else if type_array.is_some() {
                        continue;
                    } else {
                        out.insert(key.clone(), transform_schema_node_for_generator(node)?);
                    }
                    continue;
                }

                if key == "const" {
                    out.insert("enum".to_string(), Value::Array(vec![node.clone()]));
                    continue;
                }

                if key == "$schema" || key == "$id" {
                    continue;
                }
                if key == "propertyNames" {
                    continue;
                }

                let transformed = transform_schema_node_for_generator(node)?;
                out.insert(key.clone(), transformed);
            }

            if let Some(TypeArrayConversion::AnyOf { schemas, nullable }) = type_array {
                out.insert("anyOf".to_string(), Value::Array(schemas));
                if nullable {
                    out.insert("nullable".to_string(), Value::Bool(true));
                }
            }

            Ok(Value::Object(out))
        }
        _ => Ok(value.clone()),
    }
}

#[derive(Debug, Clone)]
enum TypeArrayConversion {
    Single { base_type: String, nullable: bool },
    AnyOf { schemas: Vec<Value>, nullable: bool },
}

fn type_array_to_generator_type(type_list: &[Value]) -> Result<Option<TypeArrayConversion>> {
    let has_null = type_list.iter().any(|value| value == "null");
    let mut seen = Vec::new();
    for value in type_list {
        let kind =
            value.as_str().context("type array entries must be strings in type conversion")?;
        if kind != "null" {
            seen.push(kind.to_string());
        }
    }

    match seen.as_slice() {
        [] => Ok(None),
        [kind] => Ok(Some(TypeArrayConversion::Single {
            base_type: kind.to_string(),
            nullable: has_null,
        })),
        _ => Ok(Some(TypeArrayConversion::AnyOf {
            schemas: seen.into_iter().map(|kind| json!({ "type": kind })).collect::<Vec<_>>(),
            nullable: has_null,
        })),
    }
}

fn run_openapi_generator(
    workspace: &Path,
    runtime: &GeneratorRuntimeConfig,
    generator: &str,
    spec_path: &Path,
    target: &TargetConfig,
    output_dir: &Path,
    openapi_version: &str,
) -> Result<()> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("create generator output {}", output_dir.display()))?;
    let workspace = workspace.canonicalize().context("canonicalize workspace")?;

    match runtime.runtime_type.as_str() {
        "local" => {
            let mut command_parts = runtime
                .command
                .as_deref()
                .unwrap_or("openapi-generator-cli")
                .split_whitespace()
                .collect::<Vec<_>>();

            let command_program = command_parts.first().copied().unwrap_or("openapi-generator-cli");
            let command_args = command_parts.split_off(1);

            let mut args = Vec::new();
            args.extend(command_args.into_iter().map(ToString::to_string));
            args.extend(vec![
                "generate".to_string(),
                "-i".to_string(),
                external_tool_path(
                    &spec_path
                        .canonicalize()
                        .with_context(|| format!("canonicalize {}", spec_path.display()))?,
                ),
                "-g".to_string(),
                generator.to_string(),
                "-o".to_string(),
                external_tool_path(
                    &output_dir
                        .canonicalize()
                        .with_context(|| format!("canonicalize {}", output_dir.display()))?,
                ),
            ]);
            if openapi_version.starts_with("3.1") {
                args.push("--skip-validate-spec".to_string());
            }
            if let Some(config_file) = &target.generator_config_file {
                let abs = workspace.join(config_file);
                if !abs.is_file() {
                    bail!("missing generator config file {}", abs.display());
                }
                args.push("-c".to_string());
                args.push(external_tool_path(&abs));
            }

            run_command(command_program, &args.iter().map(String::as_str).collect::<Vec<_>>())?;
        }
        "docker" => {
            if command_exists("docker").is_none() {
                bail!("docker is required for generator runtime type docker");
            }

            let spec_path = spec_path
                .canonicalize()
                .with_context(|| format!("canonicalize {}", spec_path.display()))?;
            let output_dir = output_dir
                .canonicalize()
                .with_context(|| format!("canonicalize {}", output_dir.display()))?;
            let spec_rel = spec_path
                .strip_prefix(&workspace)
                .with_context(|| format!("spec path {} outside workspace", spec_path.display()))?
                .to_string_lossy()
                .to_string()
                .replace('\\', "/");
            let out_rel = output_dir
                .strip_prefix(&workspace)
                .with_context(|| format!("output path {} outside workspace", output_dir.display()))?
                .to_string_lossy()
                .to_string()
                .replace('\\', "/");

            let mut args = vec![
                "run".to_string(),
                "--rm".to_string(),
                "-v".to_string(),
                format!("{}:/local", workspace.display()),
                runtime.image.clone(),
                "generate".to_string(),
                "-i".to_string(),
                format!("/local/{spec_rel}"),
                "-g".to_string(),
                generator.to_string(),
                "-o".to_string(),
                format!("/local/{out_rel}"),
            ];
            if openapi_version.starts_with("3.1") {
                args.push("--skip-validate-spec".to_string());
            }
            if let Some(config_file) = &target.generator_config_file {
                let full = workspace.join(config_file);
                let rel = full
                    .strip_prefix(&workspace)
                    .with_context(|| format!("config path {} outside workspace", full.display()))?
                    .to_string_lossy()
                    .to_string()
                    .replace('\\', "/");
                if !full.is_file() {
                    bail!("missing generator config file {}", full.display());
                }
                args.push("-c".to_string());
                args.push(format!("/local/{rel}"));
            }

            let extra = runtime.command.as_deref().unwrap_or("");
            if !extra.trim().is_empty() {
                args.extend(extra.split_whitespace().map(ToString::to_string));
            }

            run_command("docker", &args.iter().map(String::as_str).collect::<Vec<_>>())?;
        }
        value => bail!("unsupported generator runtime type {}", value),
    }

    Ok(())
}

fn run_command(cmd: &str, args: &[&str]) -> Result<()> {
    let program = command_program(cmd);
    let status =
        Command::new(&program).args(args).status().with_context(|| format!("spawn {cmd}"))?;
    if !status.success() {
        bail!("command failed: {} {}", cmd, args.join(" "));
    }
    Ok(())
}

fn command_program(cmd: &str) -> String {
    if cmd == "bash" {
        if let Ok(override_path) = env::var("LXMF_RS_BASH") {
            if !override_path.trim().is_empty() {
                return override_path;
            }
        }
    }
    cmd.to_string()
}

fn external_tool_path(path: &Path) -> String {
    let rendered = path.to_string_lossy();
    rendered.strip_prefix(r"\\?\").unwrap_or(&rendered).to_string()
}

fn collect_files_recursive(path: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(path).with_context(|| format!("read_dir {}", path.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", path.display()))?;
        let item = entry.path();
        if item.is_dir() {
            out.extend(collect_files_recursive(&item)?);
            continue;
        }
        if item.is_file() {
            out.push(item);
        }
    }
    Ok(out)
}
