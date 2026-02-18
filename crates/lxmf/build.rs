use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(reticulum_api_v2)");
    println!("cargo:rerun-if-changed=build.rs");

    if is_reticulum_api_v2() {
        println!("cargo:rustc-cfg=reticulum_api_v2");
    }
}

fn is_reticulum_api_v2() -> bool {
    if let Some(manifest_path) = monorepo_reticulum_manifest_path() {
        return has_text_in_reticulum_source(&manifest_path, "accept_announce_with_metadata")
            || has_text_in_reticulum_source(&manifest_path, "OutboundDeliveryOptions")
            || has_reticulum_outbound_bridge_with_options(&manifest_path);
    }

    if let Some(version) = reticulum_version() {
        if is_definitely_v2_by_version(&version) {
            return true;
        }
    }

    if let Some(manifest_path) = reticulum_manifest_path() {
        return has_text_in_reticulum_source(&manifest_path, "accept_announce_with_metadata")
            || has_text_in_reticulum_source(&manifest_path, "OutboundDeliveryOptions")
            || has_reticulum_outbound_bridge_with_options(&manifest_path);
    }

    false
}

fn monorepo_reticulum_manifest_path() -> Option<PathBuf> {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")?;
    let local_manifest = Path::new(&manifest_dir).join("../reticulum/Cargo.toml");
    local_manifest.exists().then_some(local_manifest)
}

fn reticulum_version() -> Option<String> {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")?;
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            &manifest_path.to_string_lossy(),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let metadata: Value = serde_json::from_slice(&output.stdout).ok()?;
    find_dependency_package(&metadata)?.get("version").and_then(Value::as_str).map(str::to_string)
}

fn reticulum_manifest_path() -> Option<PathBuf> {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")?;
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            &manifest_path.to_string_lossy(),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let metadata: Value = serde_json::from_slice(&output.stdout).ok()?;
    find_dependency_package(&metadata).and_then(|package: &Value| {
        package.get("manifest_path").and_then(Value::as_str).map(PathBuf::from)
    })
}

fn find_dependency_package(metadata: &Value) -> Option<&Value> {
    metadata.get("packages")?.as_array()?.iter().find(|package: &&Value| {
        package
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == "reticulum-rs" || name == "reticulum")
    })
}

fn is_definitely_v2_by_version(version: &str) -> bool {
    let mut parts = version.split('.').filter_map(|value| value.parse::<u32>().ok());
    let major = match parts.next() {
        Some(value) => value,
        None => return false,
    };
    let minor = match parts.next() {
        Some(value) => value,
        None => return false,
    };
    let patch = match parts.next() {
        Some(value) => value,
        None => return false,
    };

    major > 0 || (major == 0 && (minor > 1 || (minor == 1 && patch >= 4)))
}

fn has_text_in_reticulum_source(manifest_path: &Path, needle: &str) -> bool {
    let crate_root = manifest_path.parent();
    let Some(crate_root) = crate_root else {
        return false;
    };
    let src_dir = crate_root.join("src");
    walk_rs_files(&src_dir).into_iter().any(|source| read_and_contains(&source, needle))
}

fn has_reticulum_outbound_bridge_with_options(manifest_path: &Path) -> bool {
    let crate_root = manifest_path.parent();
    let Some(crate_root) = crate_root else {
        return false;
    };
    let src_dir = crate_root.join("src");
    walk_rs_files(&src_dir)
        .into_iter()
        .any(|source| has_outbound_bridge_v2_signature_in_file(&source))
}

fn has_outbound_bridge_v2_signature_in_file(path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };

    let Some(start_index) = content.find("trait OutboundBridge") else {
        return false;
    };

    let Some(open_offset) = content[start_index..].find('{') else {
        return false;
    };
    let open_index = start_index + open_offset;

    let mut depth = 0usize;
    let mut end_index = None;
    for (offset, ch) in content[open_index..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        end_index = Some(open_index + offset);
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    let Some(end_index) = end_index else {
        return false;
    };
    let bridge_body = &content[open_index + 1..end_index];
    bridge_body.contains("fn deliver") && bridge_body.contains("OutboundDeliveryOptions")
}

fn walk_rs_files(base: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut stack = vec![base.to_path_buf()];

    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };

        for entry in entries.flatten() {
            let file_path = entry.path();
            if file_path.is_dir() {
                if file_path.file_name().is_some_and(|name| name == ".git") {
                    continue;
                }
                stack.push(file_path);
            } else if file_path.extension().is_some_and(|ext| ext == "rs") {
                result.push(file_path);
            }
        }
    }

    result
}

fn read_and_contains(file: &Path, needle: &str) -> bool {
    fs::read_to_string(file).is_ok_and(|content| content.contains(needle))
}
