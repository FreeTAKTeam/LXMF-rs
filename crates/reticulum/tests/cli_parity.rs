use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

fn build_cli_binaries() {
    static BUILD_CLI_BINARIES: Once = Once::new();

    BUILD_CLI_BINARIES.call_once(|| {
        let status = Command::new("cargo")
            .args(["build", "--bins", "--features", "cli-tools"])
            .status()
            .expect("spawn cargo build --bins");
        assert!(status.success(), "cargo build --bins --features cli-tools failed");
    });
}

fn target_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let add_dir = |path: PathBuf, dirs: &mut Vec<PathBuf>| {
        if !dirs.contains(&path) {
            dirs.push(path);
        }
    };

    if let Ok(env_dir) = env::var("CARGO_TARGET_DIR") {
        let path = if Path::new(&env_dir).is_relative() {
            env::current_dir()
                .map(|cwd| cwd.join(&env_dir))
                .unwrap_or_else(|_| PathBuf::from(&env_dir))
        } else {
            PathBuf::from(&env_dir)
        };
        add_dir(path, &mut dirs);
    }

    if let Ok(exe) = env::current_exe() {
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
        add_dir(env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), &mut dirs);
        add_dir(PathBuf::from("target"), &mut dirs);
    }

    if dirs.iter().all(|dir| dir.file_name() != Some(OsStr::new("target"))) {
        add_dir(PathBuf::from("target"), &mut dirs);
    }

    dirs
}

fn cli_binary(name: &str) -> PathBuf {
    if let Ok(path) = std::env::var(format!("CARGO_BIN_EXE_{name}")) {
        return PathBuf::from(path);
    }

    let mut candidate_profiles = vec!["debug".to_string()];
    if let Ok(profile) = env::var("PROFILE") {
        if !candidate_profiles.contains(&profile) {
            candidate_profiles.push(profile);
        }
    }
    if !candidate_profiles.contains(&"test".to_string()) {
        candidate_profiles.push("test".to_string());
    }

    let mut exe_name = PathBuf::from(name);
    if cfg!(windows) {
        exe_name.set_extension("exe");
    }

    for target_dir in target_dirs() {
        for profile in &candidate_profiles {
            let candidate = target_dir.join(profile).join(&exe_name);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    panic!("CLI binary for '{name}' not found in any of: {:?}", target_dirs());
}

fn assert_help_contains_config(binary: &str) {
    build_cli_binaries();

    let binary = cli_binary(binary);
    let output = Command::new(&binary).arg("--help").output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--config"));
}

#[test]
fn rnstatus_help_matches_expected_flags() {
    assert_help_contains_config("rnstatus");
}

#[test]
fn rnprobe_help_matches_expected_flags() {
    assert_help_contains_config("rnprobe");
}

#[test]
fn rnpath_help_matches_expected_flags() {
    assert_help_contains_config("rnpath");
}

#[test]
fn rnid_help_matches_expected_flags() {
    assert_help_contains_config("rnid");
}

#[test]
fn rnsd_help_matches_expected_flags() {
    assert_help_contains_config("rnsd");
}

#[test]
fn rncp_help_matches_expected_flags() {
    assert_help_contains_config("rncp");
}

#[test]
fn rnx_help_matches_expected_flags() {
    assert_help_contains_config("rnx");
}

#[test]
fn rnpkg_help_matches_expected_flags() {
    assert_help_contains_config("rnpkg");
}

#[test]
fn rnodeconf_help_matches_expected_flags() {
    assert_help_contains_config("rnodeconf");
}

#[test]
fn rnir_help_matches_expected_flags() {
    assert_help_contains_config("rnir");
}
