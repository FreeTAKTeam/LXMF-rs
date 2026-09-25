use super::{
    configured_argv, read_bounded_file, read_stderr_tail, select_backend,
    wait_pipeline_with_cancel, wait_pipeline_with_status, webp_info, BACKENDS,
};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// Expected selections and argv mirror RNS/Utilities/rngit/media.py at
// Reticulum 99de23c040d507e3fefca19e87b182302902725d.
#[test]
fn automatic_backend_selection_uses_reference_preference_order() {
    for (selected, backend) in BACKENDS.iter().enumerate() {
        let available = |program: &str| {
            BACKENDS[selected..]
                .iter()
                .any(|candidate| candidate.argv[0] == program)
        };
        assert_eq!(
            select_backend(None, None, available).map(|value| value.name),
            Some(backend.name),
            "wrong automatic selection when {} is the first available backend",
            backend.name
        );
    }
    assert!(select_backend(None, None, |_| false).is_none());
}

#[cfg(unix)]
#[test]
fn backend_availability_skips_non_executable_files_like_python_which() {
    use super::executable_file;
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary directory");
    let backend = directory.path().join("magick");
    std::fs::write(&backend, b"not executable").expect("write backend placeholder");
    std::fs::set_permissions(&backend, std::fs::Permissions::from_mode(0o644))
        .expect("make backend non-executable");
    assert!(!executable_file(&backend));

    std::fs::set_permissions(&backend, std::fs::Permissions::from_mode(0o755))
        .expect("make backend executable");
    assert!(executable_file(&backend));
}

#[test]
fn explicit_backend_selection_matches_reference_override_semantics() {
    for backend in BACKENDS {
        assert_eq!(
            select_backend(Some(backend.name), None, |program| program == backend.argv[0])
                .map(|value| value.name),
            Some(backend.name),
            "explicit selection failed for {}",
            backend.name
        );
        assert!(
            select_backend(Some(backend.name), None, |_| false).is_none(),
            "unavailable explicitly selected backend must not select another family"
        );
    }
    assert_eq!(
        select_backend(Some(""), None, |program| program == "ffmpeg").map(|value| value.name),
        Some("ffmpeg"),
        "empty environment value follows Python's falsey override behavior"
    );
    assert!(select_backend(Some("unknown"), None, |_| true).is_none());
}

#[test]
fn automatic_backend_selection_prefers_previous_available_winner() {
    let first_available = |program: &str| matches!(program, "ffmpeg" | "magick");
    assert_eq!(
        select_backend(None, None, first_available).map(|backend| backend.name),
        Some("magick")
    );
    assert_eq!(
        select_backend(None, Some("ffmpeg"), first_available).map(|backend| backend.name),
        Some("ffmpeg"),
        "pinned Python moves its previous winner ahead of newly available backends"
    );
    assert_eq!(
        select_backend(None, Some("ffmpeg"), |program| program == "magick")
            .map(|backend| backend.name),
        Some("magick"),
        "an unavailable cached winner falls back to normal preference order"
    );
    assert_eq!(
        select_backend(Some("magick"), Some("ffmpeg"), first_available)
            .map(|backend| backend.name),
        Some("magick"),
        "explicit backend selection takes precedence over the automatic winner"
    );
}

#[test]
fn configured_argv_matches_pinned_python_for_every_backend_family() {
    let expected = [
        (
            "magick",
            &["magick", "-", "-quality", "85", "-resize", "640x640>", "webp:-"][..],
        ),
        (
            "convert",
            &["convert", "-", "-quality", "85", "-resize", "640x640>", "webp:-"][..],
        ),
        (
            "gm",
            &[
                "gm", "convert", "-", "-quality", "85", "-resize", "640x640>", "webp:-",
            ][..],
        ),
        (
            "ffmpeg",
            &[
                "ffmpeg",
                "-y",
                "-loglevel",
                "error",
                "-i",
                "-",
                "-quality",
                "85",
                "-vf",
                "scale='min(iw,640)':'min(ih,640)':force_original_aspect_ratio=decrease",
                "-f",
                "webp",
                "pipe:1",
            ][..],
        ),
        (
            "avconv",
            &[
                "avconv",
                "-y",
                "-loglevel",
                "error",
                "-i",
                "-",
                "-quality",
                "85",
                "-vf",
                "scale='min(iw,640)':'min(ih,640)':force_original_aspect_ratio=decrease",
                "-f",
                "webp",
                "pipe:1",
            ][..],
        ),
    ];

    for (name, expected_argv) in expected {
        let backend = BACKENDS.iter().find(|backend| backend.name == name).expect("backend");
        assert_eq!(
            configured_argv(backend, Some(85), Some(640)),
            expected_argv,
            "configured argv differs from pinned Python for {name}"
        );
    }
}

#[test]
#[ignore = "requires the pinned Python Reticulum checkout"]
fn configured_backend_argv_matches_pinned_python_helper() {
    let python_repo = std::env::var_os("RETICULUM_PY_REPO")
        .expect("set RETICULUM_PY_REPO to the pinned Reticulum checkout");
    let helper = std::path::PathBuf::from(python_repo).join("RNS/Utilities/rngit/media.py");
    assert!(helper.is_file(), "pinned rngit media helper not found: {}", helper.display());

    let script = r#"
import importlib.util
import json
import sys
import types

sys.modules["RNS"] = types.SimpleNamespace(log=lambda *_args: None, LOG_WARNING=2)
spec = importlib.util.spec_from_file_location("rngit_media_reference", sys.argv[1])
media = importlib.util.module_from_spec(spec)
spec.loader.exec_module(media)
available = {argv[0] for _name, argv in media.BACKENDS}
media.shutil.which = lambda program: program if program in available else None
configured = []
for name, _argv in media.BACKENDS:
    media._ENV_BACKEND = name
    configured.append(media._configured_backend(quality=85, max_dimension=640))
print(json.dumps(configured))
"#;
    let python = std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into());
    let output = Command::new(python)
        .arg("-c")
        .arg(script)
        .arg(&helper)
        .output()
        .expect("run pinned Python media helper");
    assert!(
        output.status.success(),
        "pinned Python media helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reference: Vec<(String, Vec<String>)> = serde_json::from_slice(&output.stdout)
        .expect("decode configured backend argv from pinned Python");
    assert_eq!(reference.len(), BACKENDS.len());
    for ((name, argv), backend) in reference.iter().zip(BACKENDS) {
        assert_eq!(name, backend.name);
        assert_eq!(argv, &configured_argv(backend, Some(85), Some(640)), "backend {name}");
    }
}

#[test]
fn configured_argv_preserves_python_clamping_and_omission_rules() {
    for backend in BACKENDS {
        let argv = configured_argv(backend, Some(0), Some(0));
        assert!(!argv.iter().any(|argument| argument == "-resize" || argument == "-vf"));
        assert!(argv.windows(2).any(|pair| pair == ["-quality", "1"]));

        let argv = configured_argv(backend, Some(u8::MAX), Some(720));
        assert!(argv.windows(2).any(|pair| pair == ["-quality", "100"]));
        if matches!(backend.name, "magick" | "convert" | "gm") {
            assert!(argv.iter().any(|argument| argument == "720x720>"));
        } else {
            assert!(argv.iter().any(|argument| argument.contains("min(iw,720)")));
        }
    }
}

#[test]
fn webp_header_dimensions_match_reference_chunk_layouts() {
    let mut vp8x = [0_u8; 30];
    vp8x[..4].copy_from_slice(b"RIFF");
    vp8x[8..12].copy_from_slice(b"WEBP");
    vp8x[12..16].copy_from_slice(b"VP8X");
    vp8x[24..27].copy_from_slice(&[9, 0, 0]);
    vp8x[27..30].copy_from_slice(&[19, 0, 0]);
    assert_eq!(webp_info(&vp8x), Some((10, 20)));

    let mut vp8l = [0_u8; 30];
    vp8l[..4].copy_from_slice(b"RIFF");
    vp8l[8..12].copy_from_slice(b"WEBP");
    vp8l[12..16].copy_from_slice(b"VP8L");
    let bits = 31_u32 | (41_u32 << 14);
    vp8l[21..25].copy_from_slice(&bits.to_le_bytes());
    assert_eq!(webp_info(&vp8l), Some((32, 42)));
}

#[test]
fn malformed_webp_headers_are_rejected() {
    assert_eq!(webp_info(&[0_u8; 30]), None);
    assert_eq!(webp_info(b"RIFF"), None);
}

#[test]
fn converted_media_read_obeys_response_limit_without_overreading() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("converted.webp");
    std::fs::write(&path, b"12345").expect("write conversion output");

    assert_eq!(read_bounded_file(&path, 5).as_deref(), Some(b"12345".as_slice()));
    assert_eq!(read_bounded_file(&path, 4), None);
}

#[cfg(unix)]
#[test]
fn webp_pipeline_timeout_terminates_and_reaps_both_processes() {
    let mut input = Command::new("/bin/sh")
        .args(["-c", "exec sleep 30"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start test blob reader");
    let input_stdout = input.stdout.take().expect("blob reader stdout");
    let input_stderr = input.stderr.take().expect("blob reader stderr");

    let mut encoder = Command::new("/bin/sh")
        .args(["-c", "exec sleep 30"])
        .stdin(input_stdout)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start test encoder");
    let encoder_stderr = encoder.stderr.take().expect("encoder stderr");

    let completed = wait_pipeline_with_cancel(
        "test-backend",
        &mut input,
        &mut encoder,
        read_stderr_tail(input_stderr),
        read_stderr_tail(encoder_stderr),
        Duration::from_millis(50),
        || false,
    );

    assert!(!completed, "a timed-out conversion pipeline must fail");
    assert!(input.try_wait().expect("wait for blob reader").is_some());
    assert!(encoder.try_wait().expect("wait for encoder").is_some());
}

#[cfg(unix)]
#[test]
fn webp_pipeline_status_error_terminates_and_reaps_both_processes() {
    for error_on_poll in [1, 2] {
        let mut input = Command::new("/bin/sh")
            .args(["-c", "exec sleep 3"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start test blob reader");
        let input_stdout = input.stdout.take().expect("blob reader stdout");
        let input_stderr = input.stderr.take().expect("blob reader stderr");

        let mut encoder = Command::new("/bin/sh")
            .args(["-c", "exec sleep 3"])
            .stdin(input_stdout)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start test encoder");
        let encoder_stderr = encoder.stderr.take().expect("encoder stderr");

        let mut polls = 0;
        let started = std::time::Instant::now();
        let completed = wait_pipeline_with_status(
            "test-backend",
            &mut input,
            &mut encoder,
            read_stderr_tail(input_stderr),
            read_stderr_tail(encoder_stderr),
            Duration::from_secs(5),
            |child| {
                polls += 1;
                if polls == error_on_poll {
                    Err(std::io::Error::other("injected child status error"))
                } else {
                    child.try_wait()
                }
            },
        );

        assert!(!completed, "a child status error must fail conversion");
        assert_eq!(polls, error_on_poll);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "status error waited for live children"
        );
        assert!(input.try_wait().expect("reap blob reader").is_some());
        assert!(encoder.try_wait().expect("reap encoder").is_some());
    }
}

#[cfg(unix)]
#[test]
fn webp_pipeline_disconnect_cancellation_terminates_and_reaps_children() {
    let mut input = Command::new("/bin/sh")
        .args(["-c", "exec sleep 30"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start test blob reader");
    let input_stdout = input.stdout.take().expect("blob reader stdout");
    let input_stderr = input.stderr.take().expect("blob reader stderr");
    let mut encoder = Command::new("/bin/sh")
        .args(["-c", "exec sleep 30"])
        .stdin(input_stdout)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start test encoder");
    let encoder_stderr = encoder.stderr.take().expect("encoder stderr");
    let started = Instant::now();
    let mut cancellation_checks = 0;

    let completed = wait_pipeline_with_cancel(
        "test-backend",
        &mut input,
        &mut encoder,
        read_stderr_tail(input_stderr),
        read_stderr_tail(encoder_stderr),
        Duration::from_secs(5),
        || {
            cancellation_checks += 1;
            cancellation_checks > 2
        },
    );

    assert!(!completed, "a disconnected Link must cancel conversion");
    assert!(cancellation_checks >= 3, "cancellation signal was not polled");
    assert!(started.elapsed() < Duration::from_secs(1), "cancellation waited for child timeout");
    assert!(input.try_wait().expect("reap blob reader").is_some());
    assert!(encoder.try_wait().expect("reap encoder").is_some());
}
