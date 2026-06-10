use std::fs;
use std::path::Path;

fn main() {
    let version_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../VERSION");
    println!("cargo:rerun-if-changed={}", version_path.display());

    let version = fs::read_to_string(&version_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", version_path.display()));
    let version = version.trim();
    assert!(!version.is_empty(), "{} must not be empty", version_path.display());
    assert!(
        !version.chars().any(char::is_whitespace),
        "{} must contain exactly one version token",
        version_path.display()
    );
    println!("cargo:rustc-env=LXMF_RS_VERSION={version}");
}
