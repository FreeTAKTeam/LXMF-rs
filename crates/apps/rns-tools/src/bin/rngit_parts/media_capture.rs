use super::{
    configured_argv, join_stderr, read_stderr_tail, selected_backend, terminate, valid_webp, wait_encoder,
    MEDIA_CONVERSION_TIMEOUT,
};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

pub(super) const MEDIA_OUTPUT_LIMIT: usize = 32 * 1024 * 1024;

pub(super) fn capture_bounded_output(
    mut reader: impl Read + Send + 'static,
    mut output: File,
) -> (JoinHandle<io::Result<()>>, std::sync::Arc<AtomicBool>) {
    let failed = std::sync::Arc::new(AtomicBool::new(false));
    let thread_failed = failed.clone();
    let handle = std::thread::spawn(move || {
        let mut captured = 0_usize;
        let mut buffer = [0_u8; 8192];
        loop {
            // Read at most one byte beyond the cap to distinguish exact-limit
            // output from overflow, without ever writing that extra byte.
            let remaining = MEDIA_OUTPUT_LIMIT.saturating_sub(captured);
            let max_read = remaining.saturating_add(1).min(buffer.len());
            let read = match reader.read(&mut buffer[..max_read]) {
                Ok(read) => read,
                Err(error) => {
                    thread_failed.store(true, Ordering::Release);
                    return Err(error);
                }
            };
            if read == 0 {
                return output.flush();
            }
            if read > remaining {
                thread_failed.store(true, Ordering::Release);
                return Ok(());
            }
            if let Err(error) = output.write_all(&buffer[..read]) {
                thread_failed.store(true, Ordering::Release);
                return Err(error);
            }
            captured += read;
        }
    });
    (handle, failed)
}

#[allow(dead_code)]
pub(super) fn convert_file_to_webp(
    source_path: &Path,
    quality: Option<u8>,
    max_dimension: Option<u32>,
    timeout: Option<Duration>,
) -> Option<PathBuf> {
    let backend = selected_backend()?;
    let encoder_argv = configured_argv(backend, quality, max_dimension);
    let input = File::open(source_path).ok()?;
    let temporary = tempfile::Builder::new()
        .prefix("rngit-media-")
        .suffix(".webp")
        .tempfile()
        .ok()?
        .into_temp_path();
    let output_path = temporary.to_path_buf();
    let output = File::create(&output_path).ok()?;
    let mut encoder_command = Command::new(&encoder_argv[0]);
    encoder_command
        .args(&encoder_argv[1..])
        .stdin(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut encoder = encoder_command.spawn().ok()?;
    let Some(stderr) = encoder.stderr.take().map(read_stderr_tail) else {
        terminate(&mut encoder);
        return None;
    };
    let Some(stdout) = encoder.stdout.take() else {
        terminate(&mut encoder);
        let _ = join_stderr(stderr);
        return None;
    };
    let (capture, capture_failed) = capture_bounded_output(stdout, output);
    if !wait_encoder(
        backend.name,
        &mut encoder,
        stderr,
        &capture_failed,
        timeout.unwrap_or(MEDIA_CONVERSION_TIMEOUT),
    ) || !capture.join().is_ok_and(|result| result.is_ok())
        || capture_failed.load(Ordering::Acquire)
        || !valid_webp(&output_path)
    {
        let _ = fs::remove_file(&output_path);
        return None;
    }
    temporary.keep().ok()
}
