#[cfg(target_os = "macos")]
fn main() {
    use std::path::PathBuf;

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let plist = manifest_dir.join("resources").join("rnx-macos-Info.plist");
    println!("cargo:rerun-if-changed={}", plist.display());
    println!(
        "cargo:rustc-link-arg-bin=rnx=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        plist.display()
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {}
