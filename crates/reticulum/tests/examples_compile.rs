use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

fn target_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let add_dir = |path: PathBuf, dirs: &mut Vec<PathBuf>| {
        if !dirs.contains(&path) {
            dirs.push(path);
        }
    };

    if let Ok(env_dir) = std::env::var("CARGO_TARGET_DIR") {
        let path = if Path::new(&env_dir).is_relative() {
            std::env::current_dir()
                .map(|cwd| cwd.join(&env_dir))
                .unwrap_or_else(|_| PathBuf::from(&env_dir))
        } else {
            PathBuf::from(&env_dir)
        };
        add_dir(path, &mut dirs);
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut current = exe.parent();
        while let Some(dir) = current {
            if dir.file_name() == Some(OsStr::new("deps")) {
                if let Some(profile_dir) = dir.parent() {
                    add_dir(profile_dir.to_path_buf(), &mut dirs);
                    if let Some(target_dir) = profile_dir.parent() {
                        add_dir(target_dir.to_path_buf(), &mut dirs);
                    }
                }
            } else if dir.file_name() == Some(OsStr::new("target")) {
                add_dir(dir.to_path_buf(), &mut dirs);
                break;
            }
            current = dir.parent();
        }
    }

    if dirs.is_empty() {
        add_dir(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), &mut dirs);
        add_dir(PathBuf::from("target"), &mut dirs);
    }

    if dirs.iter().all(|dir| dir.file_name() != Some(OsStr::new("target"))) {
        add_dir(PathBuf::from("target"), &mut dirs);
    }

    dirs
}

fn example_path(target_dir: &Path, profiles: &[String], example: &str) -> Option<PathBuf> {
    for profile in profiles {
        let mut path = target_dir.join(profile).join("examples").join(example);
        if cfg!(windows) {
            path.set_extension("exe");
        }
        if path.exists() {
            return Some(path);
        }
    }
    None
}

#[test]
fn examples_compile() {
    const EXAMPLES: &[&str] =
        &["tcp_server", "tcp_client", "udp_link", "link_client", "testnet_client", "multihop"];
    static BUILD_EXAMPLES: Once = Once::new();

    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let target_dirs = target_dirs();

    let mut profiles = vec![profile.clone()];
    if profile != "debug" {
        profiles.push("debug".into());
    }

    let missing_examples: HashSet<&str> = EXAMPLES
        .iter()
        .copied()
        .filter(|example| {
            let mut found = false;

            for target_dir in target_dirs.iter() {
                if example_path(target_dir, &profiles, example).is_some() {
                    found = true;
                    break;
                }
            }

            !found
        })
        .collect();

    if !missing_examples.is_empty() {
        BUILD_EXAMPLES.call_once(|| {
            let status = Command::new("cargo")
                .args(["build", "--examples"])
                .status()
                .expect("spawn cargo build --examples");
            assert!(status.success(), "cargo build --examples failed");
        });
    }

    for example in missing_examples {
        assert!(
            target_dirs
                .iter()
                .any(|target_dir| example_path(target_dir, &profiles, example).is_some()),
            "example binary '{example}' missing in any candidate profile ({:?}) under {}",
            profiles,
            target_dirs
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}
