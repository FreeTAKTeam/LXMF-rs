fn run_unused_deps() -> Result<()> {
    let rustup_available = Command::new("rustup")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    if rustup_available {
        let nightly_udeps = toolchain_cargo_command("nightly", "udeps --workspace --all-targets");
        return run("bash", &["-lc", &nightly_udeps]);
    }

    run("cargo", &["+nightly", "udeps", "--workspace", "--all-targets"])
}

fn run_migration_checks() -> Result<()> {
    let enforce_legacy_imports =
        std::env::var("ENFORCE_LEGACY_APP_IMPORTS").unwrap_or("1".to_string());
    let enforce_legacy_shims =
        std::env::var("ENFORCE_RETM_LEGACY_SHIMS").unwrap_or("1".to_string());
    run_sdk_migration_check()?;
    run_boundary_checks(&enforce_legacy_imports, &enforce_legacy_shims)?;
    run(
        "bash",
        &["-lc", "! grep -RInE 'crates/(lxmf|reticulum|reticulum-daemon)/' README.md .github/workflows || exit 1"],
    )?;
    Ok(())
}

fn run_architecture_checks() -> Result<()> {
    run_architecture_lint_check()?;
    run_module_size_check()
}

fn run_forbidden_deps() -> Result<()> {
    let enforce_legacy_imports =
        std::env::var("ENFORCE_LEGACY_APP_IMPORTS").unwrap_or("1".to_string());
    let enforce_legacy_shims =
        std::env::var("ENFORCE_RETM_LEGACY_SHIMS").unwrap_or("1".to_string());
    run_boundary_checks(&enforce_legacy_imports, &enforce_legacy_shims)
}

fn run_architecture_lint_check() -> Result<()> {
    run_forbidden_deps()?;

    let report = fs::read_to_string(ARCH_BOUNDARY_REPORT_PATH).with_context(|| {
        format!("missing architecture boundary report at {ARCH_BOUNDARY_REPORT_PATH}")
    })?;
    for marker in [
        "# Architecture Boundary Report",
        "## Allowed library edges",
        "## Actual library edges",
        "## Allowed app edges",
        "## Actual app edges",
    ] {
        if !report.contains(marker) {
            bail!("architecture boundary report missing marker '{marker}'");
        }
    }

    Ok(())
}

fn run_boundary_checks(enforce_legacy_imports: &str, enforce_legacy_shims: &str) -> Result<()> {
    let command = format!(
        "ENFORCE_LEGACY_APP_IMPORTS={enforce_legacy_imports} ENFORCE_RETM_LEGACY_SHIMS={enforce_legacy_shims} ./tools/scripts/check-boundaries.sh"
    );
    run("bash", &["-lc", &command])
}

fn run_module_size_check() -> Result<()> {
    run("bash", &["tools/scripts/check-module-size.sh"])
}

fn parse_cutover_rows(markdown: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut in_table = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if !in_table {
            if trimmed.starts_with("| Surface |")
                && trimmed.contains("| Classification |")
                && trimmed.contains("| Removal version |")
            {
                in_table = true;
            }
            continue;
        }

        if !trimmed.starts_with('|') {
            if !rows.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.contains("---") {
            continue;
        }

        let cells = trimmed
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_string())
            .collect::<Vec<_>>();
        if cells.len() != 7 {
            bail!("malformed cutover row '{trimmed}' (expected 7 columns, found {})", cells.len());
        }
        rows.push(cells);
    }

    Ok(rows)
}

fn parse_markdown_table_rows(markdown: &str, header_cells: &[&str]) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut in_table = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if !in_table {
            if trimmed.starts_with('|')
                && header_cells.iter().all(|header_cell| trimmed.contains(header_cell))
            {
                in_table = true;
            }
            continue;
        }

        if !trimmed.starts_with('|') {
            if !rows.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.contains("---") {
            continue;
        }

        let cells = trimmed
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_string())
            .collect::<Vec<_>>();
        rows.push(cells);
    }

    Ok(rows)
}

fn extract_backtick_value(document: &str, marker: &str) -> Option<String> {
    for line in document.lines() {
        if !line.contains(marker) {
            continue;
        }
        let start = line.find('`')?;
        let rest = &line[start + 1..];
        let end = rest.find('`')?;
        return Some(rest[..end].trim().to_string());
    }
    None
}

fn capture_public_api(manifest: &str) -> Result<String> {
    let toolchain = public_api_toolchain();
    let args = format!("public-api --manifest-path {manifest} -sss --color never");
    let command = toolchain_cargo_command(&toolchain, &args);
    let output = Command::new(command_program("bash"))
        .args(["-lc", &command])
        .output()
        .with_context(|| format!("failed to spawn cargo public-api for {manifest}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cargo public-api failed for {manifest}: {stderr}");
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("cargo public-api output was not valid utf-8 for {manifest}"))
}

fn public_api_toolchain() -> String {
    std::env::var("SDK_API_BREAK_TOOLCHAIN").unwrap_or_else(|_| "nightly".to_string())
}

fn toolchain_cargo_command(toolchain: &str, cargo_args: &str) -> String {
    format!(
        "set -euo pipefail; \
         CARGO_BIN=\"$(rustup which --toolchain {toolchain} cargo)\"; \
         RUSTC_BIN=\"$(rustup which --toolchain {toolchain} rustc)\"; \
         RUSTDOC_BIN=\"$(rustup which --toolchain {toolchain} rustdoc)\"; \
         PATH=\"$(dirname \"$CARGO_BIN\"):$PATH\" \
         RUSTUP_TOOLCHAIN={toolchain} \
         RUSTC=\"$RUSTC_BIN\" \
         RUSTDOC=\"$RUSTDOC_BIN\" \
         \"$CARGO_BIN\" {cargo_args}"
    )
}

fn normalize_public_api(raw: &str) -> String {
    raw.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("warning:"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let program = command_program(cmd);
    let status = Command::new(&program)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn {cmd}"))?;
    if !status.success() {
        bail!("command failed: {cmd} {}", args.join(" "));
    }
    Ok(())
}

fn command_program(cmd: &str) -> String {
    if cmd == "bash" {
        if let Ok(override_path) = std::env::var("LXMF_RS_BASH") {
            if !override_path.trim().is_empty() {
                return override_path;
            }
        }
        if let Some(git_bash) = default_git_bash() {
            return git_bash;
        }
    }
    cmd.to_string()
}

#[cfg(windows)]
fn default_git_bash() -> Option<String> {
    [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
    ]
    .into_iter()
    .find(|candidate| Path::new(candidate).exists())
    .map(str::to_string)
}

#[cfg(not(windows))]
fn default_git_bash() -> Option<String> {
    None
}

fn run_publish_crates(wave: PublishWave, dry_run: bool, allow_dirty: bool) -> Result<()> {
    for krate in publish_wave_crates(wave) {
        log::info!("publishing {} from {}", krate.package, krate.manifest_path);
        if dry_run {
            run_publish_dry_run_with_fallback(*krate, allow_dirty)?;
        } else {
            let mut args = vec!["publish"];
            if allow_dirty {
                args.push("--allow-dirty");
            }
            args.push("--manifest-path");
            args.push(krate.manifest_path);
            run("cargo", &args)?;
        }
    }
    Ok(())
}

fn run_yank_crate(package: &str, version: &str, undo: bool) -> Result<()> {
    let mut args = vec!["yank"];
    if undo {
        args.push("--undo");
    }
    args.push("--vers");
    args.push(version);
    args.push(package);
    run("cargo", &args)
}

fn publish_wave_crates(wave: PublishWave) -> &'static [PublishedCrate] {
    match wave {
        PublishWave::Wave1 => WAVE1_PUBLIC_CRATES,
        PublishWave::Facades => FACADE_PUBLIC_CRATES,
        PublishWave::All => {
            static ALL_PUBLIC_CRATES: &[PublishedCrate] = &[
                PublishedCrate {
                    package: "lxmf-reference",
                    manifest_path: "crates/libs/lxmf-reference/Cargo.toml",
                },
                PublishedCrate {
                    package: "reticulum-rs-core",
                    manifest_path: "crates/libs/rns-core/Cargo.toml",
                },
                PublishedCrate {
                    package: "lxmf-wire",
                    manifest_path: "crates/libs/lxmf-core/Cargo.toml",
                },
                PublishedCrate {
                    package: "reticulum-rs-transport",
                    manifest_path: "crates/libs/rns-transport/Cargo.toml",
                },
                PublishedCrate {
                    package: "reticulum-rs-rpc",
                    manifest_path: "crates/libs/rns-rpc/Cargo.toml",
                },
                PublishedCrate {
                    package: "lxmf-sdk",
                    manifest_path: "crates/libs/lxmf-sdk/Cargo.toml",
                },
                PublishedCrate {
                    package: "reticulum-rs",
                    manifest_path: "crates/libs/reticulum-rs/Cargo.toml",
                },
                PublishedCrate { package: "lxmf", manifest_path: "crates/libs/lxmf/Cargo.toml" },
            ];
            ALL_PUBLIC_CRATES
        }
    }
}

fn run_publish_dry_run_with_fallback(krate: PublishedCrate, allow_dirty: bool) -> Result<()> {
    let mut args = vec!["publish", "--dry-run"];
    if allow_dirty {
        args.push("--allow-dirty");
    }
    args.push("--manifest-path");
    args.push(krate.manifest_path);

    let output = Command::new("cargo")
        .args(&args)
        .output()
        .with_context(|| format!("failed to spawn cargo publish for {}", krate.package))?;
    print_cargo_output(&output);
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("failed to select a version for the requirement")
        || stderr.contains("no matching package named")
    {
        log::warn!(
            "dry-run fallback: {} depends on unpublished local versions; validating package contents instead",
            krate.package
        );
        let mut package_args = vec!["package", "--list"];
        if allow_dirty {
            package_args.push("--allow-dirty");
        }
        package_args.push("--manifest-path");
        package_args.push(krate.manifest_path);
        return run("cargo", &package_args);
    }

    bail!("command failed: cargo {}", args.join(" "));
}

fn print_cargo_output(output: &std::process::Output) {
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        log::error!("{}", String::from_utf8_lossy(&output.stderr));
    }
}

#[cfg(test)]
mod version_tests {
    use super::{public_api_line_diff, resolve_release_version};

    #[test]
    fn public_api_diff_reports_removed_and_added_lines() {
        assert_eq!(
            public_api_line_diff(
                "pub const OLD: &str\npub struct Shared",
                "pub const NEW: &str\npub struct Shared"
            ),
            "- pub const OLD: &str\n+ pub const NEW: &str"
        );
    }

    #[test]
    fn public_api_diff_reports_ordering_changes() {
        assert_eq!(
            public_api_line_diff("pub const A: &str\npub const B: &str", "pub const B: &str\npub const A: &str"),
            "first ordering difference at line 1:\nbaseline: pub const A: &str\ncurrent: pub const B: &str"
        );
    }

    #[test]
    fn explicit_release_version_wins() {
        assert_eq!(
            resolve_release_version(Some(" custom/1 "), Some("v0.2.3"), "0.2.3")
                .expect("explicit version"),
            "custom-1"
        );
    }

    #[test]
    fn matching_exact_tag_is_used() {
        assert_eq!(
            resolve_release_version(None, Some("v0.2.3"), "0.2.3").expect("matching tag"),
            "v0.2.3"
        );
    }

    #[test]
    fn mismatched_exact_tag_is_rejected() {
        let error = resolve_release_version(None, Some("v0.2.4"), "0.2.3")
            .expect_err("mismatched tag must fail");
        assert!(error.to_string().contains("does not match project VERSION"));
    }

    #[test]
    fn project_version_is_used_without_exact_tag() {
        assert_eq!(
            resolve_release_version(None, None, "0.2.3\n").expect("VERSION fallback"),
            "0.2.3"
        );
    }
}
