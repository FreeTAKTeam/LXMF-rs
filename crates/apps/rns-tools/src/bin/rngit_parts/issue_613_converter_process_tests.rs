use super::convert_to_webp_with_cancel;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

struct EnvironmentGuard(Vec<(&'static str, Option<OsString>)>);

impl EnvironmentGuard {
    fn set(values: &[(&'static str, OsString)]) -> Self {
        let previous = values
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in values {
            std::env::set_var(key, value);
        }
        Self(previous)
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[test]
fn converter_process_boundary_honors_configuration_output_and_failures() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let backend_dir = temporary.path().join("backend");
    fs::create_dir(&backend_dir).expect("create fake backend directory");
    let args_path = temporary.path().join("args");
    let input_path = temporary.path().join("input");
    let webp_path = temporary.path().join("fixture.webp");
    let executable = backend_dir.join("ffmpeg");
    fs::write(
        &executable,
        b"#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$RNGIT_TEST_ARGS\"\ncat > \"$RNGIT_TEST_INPUT\"\ncase \"$RNGIT_TEST_MODE\" in\n  success) cat \"$RNGIT_TEST_WEBP\" ;;\n  failed-status) cat \"$RNGIT_TEST_WEBP\"; printf 'injected encoder failure' >&2; exit 23 ;;\n  timeout) exec sleep 5 ;;\nesac\n",
    )
    .expect("write fake converter");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .expect("make fake converter executable");

    let mut webp = [0_u8; 30];
    webp[..4].copy_from_slice(b"RIFF");
    webp[4..8].copy_from_slice(&22_u32.to_le_bytes());
    webp[8..12].copy_from_slice(b"WEBP");
    webp[12..16].copy_from_slice(b"VP8X");
    webp[16..20].copy_from_slice(&10_u32.to_le_bytes());
    fs::write(&webp_path, webp).expect("write minimal WebP header fixture");

    let mut search_paths = vec![backend_dir];
    search_paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(search_paths).expect("join test PATH");
    let _environment = EnvironmentGuard::set(&[
        ("PATH", path),
        ("RNGIT_MEDIA_BACKEND", OsString::from("ffmpeg")),
        ("RNGIT_TEST_ARGS", args_path.clone().into_os_string()),
        ("RNGIT_TEST_INPUT", input_path.clone().into_os_string()),
        ("RNGIT_TEST_WEBP", webp_path.clone().into_os_string()),
        ("RNGIT_TEST_MODE", OsString::from("success")),
    ]);

    let output_path = temporary.path().join("converted.webp");
    let input = ["/bin/sh", "-c", "printf source-bytes"]
        .map(str::to_owned)
        .to_vec();
    assert!(convert_to_webp_with_cancel(
        &input,
        &output_path,
        None,
        Some(Duration::from_secs(1)),
        Some(37),
        Some(321),
        || false,
    ));
    assert_eq!(fs::read(&output_path).expect("read accepted output"), webp);
    assert_eq!(fs::read(&input_path).expect("read converter stdin"), b"source-bytes");
    let args = fs::read_to_string(&args_path).expect("read actual converter arguments");
    let args = args.lines().collect::<Vec<_>>();
    assert!(args.windows(2).any(|pair| pair == ["-quality", "37"]), "{args:?}");
    assert!(args.windows(2).any(|pair| {
        pair == [
            "-vf",
            "scale='min(iw,321)':'min(ih,321)':force_original_aspect_ratio=decrease",
        ]
    }), "{args:?}");

    std::env::set_var("RNGIT_TEST_MODE", "failed-status");
    assert!(!convert_to_webp_with_cancel(
        &input,
        &output_path,
        None,
        Some(Duration::from_secs(1)),
        Some(37),
        Some(321),
        || false,
    ));
    assert!(!output_path.exists(), "a valid-looking output cannot override nonzero status");

    std::env::set_var("RNGIT_TEST_MODE", "timeout");
    let started = Instant::now();
    assert!(!convert_to_webp_with_cancel(
        &input,
        &output_path,
        None,
        Some(Duration::from_millis(100)),
        Some(37),
        Some(321),
        || false,
    ));
    assert!(started.elapsed() < Duration::from_secs(2), "converter exceeded its bound");
    assert!(!output_path.exists(), "timed-out output must be discarded");

}
