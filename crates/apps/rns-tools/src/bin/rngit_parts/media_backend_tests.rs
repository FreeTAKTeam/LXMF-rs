use super::{
    configured_argv, read_stderr_tail, select_backend, wait_pipeline, webp_info, BACKENDS,
};
use std::process::{Command, Stdio};
use std::time::Duration;

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
            select_backend(None, available).map(|value| value.name),
            Some(backend.name),
            "wrong automatic selection when {} is the first available backend",
            backend.name
        );
    }
    assert!(select_backend(None, |_| false).is_none());
}

#[test]
fn explicit_backend_selection_matches_reference_override_semantics() {
    for backend in BACKENDS {
        assert_eq!(
            select_backend(Some(backend.name), |program| program == backend.argv[0])
                .map(|value| value.name),
            Some(backend.name),
            "explicit selection failed for {}",
            backend.name
        );
        assert!(
            select_backend(Some(backend.name), |_| false).is_none(),
            "unavailable explicitly selected backend must not select another family"
        );
    }
    assert_eq!(
        select_backend(Some(""), |program| program == "ffmpeg").map(|value| value.name),
        Some("ffmpeg"),
        "empty environment value follows Python's falsey override behavior"
    );
    assert!(select_backend(Some("unknown"), |_| true).is_none());
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

    let completed = wait_pipeline(
        "test-backend",
        &mut input,
        &mut encoder,
        read_stderr_tail(input_stderr),
        read_stderr_tail(encoder_stderr),
        Duration::from_millis(50),
    );

    assert!(!completed, "a timed-out conversion pipeline must fail");
    assert!(input.try_wait().expect("wait for blob reader").is_some());
    assert!(encoder.try_wait().expect("wait for encoder").is_some());
}
