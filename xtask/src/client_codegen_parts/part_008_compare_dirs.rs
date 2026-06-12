fn compare_dirs(expected: &Path, actual: &Path) -> Result<()> {
    if !actual.is_dir() {
        bail!("generated target missing {}", actual.display());
    }

    let expected_map = collect_file_hashes(expected)?;
    let actual_map = collect_file_hashes(actual)?;

    if expected_map == actual_map {
        return Ok(());
    }

    let missing =
        expected_map.keys().filter(|path| !actual_map.contains_key(*path)).collect::<Vec<_>>();
    let extra =
        actual_map.keys().filter(|path| !expected_map.contains_key(*path)).collect::<Vec<_>>();

    if !missing.is_empty() || !extra.is_empty() {
        bail!("output mismatch: missing {:?}, extra {:?}", missing, extra);
    }

    let mut mismatched = Vec::new();
    for (path, expected_hash) in expected_map {
        let actual_hash = actual_map.get(&path).context(format!("missing hash for {path}"))?;
        if expected_hash != *actual_hash {
            mismatched.push(path);
        }
    }

    if !mismatched.is_empty() {
        bail!("generated output drift: {:?}", mismatched);
    }

    Ok(())
}

fn sync_dirs(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        fs::remove_dir_all(dst).with_context(|| format!("remove {}", dst.display()))?;
    }
    fs::create_dir_all(dst).with_context(|| format!("create {}", dst.display()))?;
    copy_dir_recursively(src, dst)
}

fn copy_dir_recursively(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src).with_context(|| format!("read_dir {}", src.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", src.display()))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)
                .with_context(|| format!("create dir {}", dst_path.display()))?;
            copy_dir_recursively(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            fs::copy(&src_path, &dst_path)
                .with_context(|| format!("copy {}", src_path.display()))?;
        }
    }
    Ok(())
}

fn collect_file_hashes(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path).with_context(|| format!("read_dir {}", path.display()))? {
            let entry = entry.with_context(|| format!("read entry {}", path.display()))?;
            let entry_path = entry.path();
            let rel = entry_path
                .strip_prefix(dir)
                .context("strip_prefix")?
                .to_string_lossy()
                .replace('\\', "/");

            if entry_path.is_dir() {
                if rel.ends_with("/__pycache__") || rel == "__pycache__" {
                    continue;
                }
                stack.push(entry_path);
                continue;
            }

            let bytes = fs::read(&entry_path)
                .with_context(|| format!("read file {}", entry_path.display()))?;
            out.insert(rel, sha256_hex(&bytes));
        }
    }

    Ok(out)
}

fn directory_hash(dir: &Path) -> Result<String> {
    let files = collect_file_hashes(dir)?;
    let mut rendered = String::new();
    for (path, hash) in files {
        rendered.push_str(&path);
        rendered.push('\n');
        rendered.push_str(&hash);
        rendered.push('\n');
    }
    Ok(sha256_hex(rendered.as_bytes()))
}

fn write_spec_hash_file(workspace: &Path, hash: &str) -> Result<()> {
    let path = workspace.join(DEFAULT_SPEC_HASH_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", path.display()))?;
    }
    fs::write(&path, format!("{hash}\n")).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn command_exists(cmd: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|dir| if cfg!(windows) { dir.join(format!("{cmd}.exe")) } else { dir.join(cmd) })
            .find(|candidate| candidate.exists())
    })
}

fn map_generator_from_language(language: &str) -> Option<String> {
    match language {
        "go" => Some("go".to_string()),
        "javascript" => Some("typescript-axios".to_string()),
        "typescript" => Some("typescript-axios".to_string()),
        "python" => Some("python".to_string()),
        _ => None,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn to_pascal_case(input: &str) -> String {
    let mut out = String::new();
    for part in input.split(|c: char| !c.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars.map(|ch| ch.to_ascii_lowercase()));
        }
    }

    if out.is_empty() {
        return "Method".to_string();
    }

    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }

    out
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(base: &Path) -> Result<Self> {
        let mut base_dir = base.join(".tmp").join("schema-client");
        let unique = format!(
            "lxmf-schema-client-generate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_else(|_| Default::default())
                .as_nanos(),
        );
        base_dir.push(unique);
        fs::create_dir_all(&base_dir)
            .with_context(|| format!("create temp dir {}", base_dir.display()))?;
        Ok(Self { path: base_dir })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
