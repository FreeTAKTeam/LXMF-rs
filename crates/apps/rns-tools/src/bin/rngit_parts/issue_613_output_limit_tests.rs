use super::{
    available_backends, capture_bounded_output, convert_to_webp_with_cancel, MEDIA_TEST_ENV_LOCK,
};
use std::ffi::OsString;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{Duration, Instant};

struct ZeroReader(usize);

impl Read for ZeroReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.0.min(output.len());
        output[..count].fill(0);
        self.0 -= count;
        Ok(count)
    }
}

struct RestoreEnvironment(Option<OsString>, Option<OsString>);

impl RestoreEnvironment {
    fn set(path: OsString, backend: OsString) -> Self {
        let old_path = std::env::var_os("PATH");
        let old_backend = std::env::var_os("RNGIT_MEDIA_BACKEND");
        std::env::set_var("PATH", path);
        std::env::set_var("RNGIT_MEDIA_BACKEND", backend);
        Self(old_path, old_backend)
    }
}

impl Drop for RestoreEnvironment {
    fn drop(&mut self) {
        if let Some(value) = self.0.take() {
            std::env::set_var("PATH", value);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(value) = self.1.take() {
            std::env::set_var("RNGIT_MEDIA_BACKEND", value);
        } else {
            std::env::remove_var("RNGIT_MEDIA_BACKEND");
        }
    }
}

#[test]
fn oversized_fake_backend_is_bounded_terminated_and_cleaned_up() {
    let _environment_lock = MEDIA_TEST_ENV_LOCK.lock().expect("media test environment lock");
    let temporary = tempfile::tempdir().expect("temporary directory");
    let backend_dir = temporary.path().join("backend");
    std::fs::create_dir(&backend_dir).expect("backend directory");
    let pid_path = temporary.path().join("backend.pid");
    let backend = backend_dir.join("ffmpeg");
    std::fs::write(
        &backend,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$$\" > '{}'\nexec head -c 33554433 /dev/zero\n",
            pid_path.display()
        ),
    )
    .expect("write oversized fake backend");
    std::fs::set_permissions(&backend, std::fs::Permissions::from_mode(0o755))
        .expect("make fake backend executable");

    let mut search_paths = vec![backend_dir];
    search_paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(search_paths).expect("join PATH");
    let _restore = RestoreEnvironment::set(path, OsString::from("ffmpeg"));
    assert!(available_backends().iter().any(|(name, available)| name == "ffmpeg" && *available));

    let output_path = temporary.path().join("converted.webp");
    let input_pid_path = temporary.path().join("input.pid");
    let started = Instant::now();
    assert!(!convert_to_webp_with_cancel(
        &[
            "/bin/sh".to_string(),
            "-c".to_string(),
            format!("printf '%s\\n' \"$$\" > '{}'; exec sleep 30", input_pid_path.display()),
        ],
        &output_path,
        None,
        None,
        None,
        None,
        || false,
    ));
    assert!(started.elapsed() < Duration::from_secs(2), "oversized conversion was not terminated promptly");
    assert!(!output_path.exists(), "oversized partial output must be removed");

    let pid = std::fs::read_to_string(&pid_path).expect("fake backend recorded its PID");
    assert!(
        !Command::new("kill").args(["-0", pid.trim()]).status().expect("check fake backend PID").success(),
        "oversized backend process must be reaped"
    );
    let input_pid = std::fs::read_to_string(&input_pid_path).expect("input process recorded its PID");
    assert!(
        !Command::new("kill").args(["-0", input_pid.trim()]).status().expect("check input process PID").success(),
        "input process must be reaped after output overflow"
    );
}

#[test]
fn bounded_capture_never_writes_the_overflow_byte() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let output_path = temporary.path().join("bounded-output");
    let output = std::fs::File::create(&output_path).expect("create output file");
    let oversized = ZeroReader(super::media_capture::MEDIA_OUTPUT_LIMIT + 1);
    let (capture, failed) = capture_bounded_output(oversized, output);

    assert!(capture.join().expect("capture worker thread").is_ok());
    assert!(failed.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(
        std::fs::metadata(output_path).expect("captured output metadata").len(),
        super::media_capture::MEDIA_OUTPUT_LIMIT as u64
    );
}
