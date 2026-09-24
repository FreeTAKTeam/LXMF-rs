use std::env as media_env;
use std::fs::File as MediaFile;
use std::process::Child as MediaChild;
use std::thread::JoinHandle as MediaJoinHandle;

const MEDIA_CONVERSION_TIMEOUT: Duration = Duration::from_secs(8);
const STDERR_LIMIT: usize = 1024;
static SELECTED_BACKEND: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

fn read_bounded_file(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let file = MediaFile::open(path).ok()?;
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    file.take((limit as u64).saturating_add(1)).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

struct Backend {
    name: &'static str,
    argv: &'static [&'static str],
}

const BACKENDS: &[Backend] = &[
    Backend { name: "magick", argv: &["magick", "-", "webp:-"] },
    Backend { name: "convert", argv: &["convert", "-", "webp:-"] },
    Backend { name: "gm", argv: &["gm", "convert", "-", "webp:-"] },
    Backend {
        name: "ffmpeg",
        argv: &["ffmpeg", "-y", "-loglevel", "error", "-i", "-", "-f", "webp", "pipe:1"],
    },
    Backend {
        name: "avconv",
        argv: &["avconv", "-y", "-loglevel", "error", "-i", "-", "-f", "webp", "pipe:1"],
    },
];

fn command_available(program: &str) -> bool {
    let program_path = Path::new(program);
    if program_path.components().count() > 1 {
        return executable_file(program_path);
    }
    let Some(path) = media_env::var_os("PATH") else { return false };
    media_env::split_paths(&path).any(|directory| {
        let candidate = directory.join(program);
        #[cfg(windows)]
        {
            executable_file(&candidate)
                || [".exe", ".cmd", ".bat"].iter().any(|suffix| {
                    executable_file(&directory.join(format!("{program}{suffix}")))
                })
        }
        #[cfg(not(windows))]
        executable_file(&candidate)
    })
}

fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else { return false };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[allow(dead_code)]
pub(crate) fn available_backends() -> Vec<(String, bool)> {
    BACKENDS
        .iter()
        .map(|backend| (backend.name.to_string(), command_available(backend.argv[0])))
        .collect()
}

fn selected_backend() -> Option<&'static Backend> {
    let requested = media_env::var("RNGIT_MEDIA_BACKEND").ok();
    let mut previous = SELECTED_BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let selected = select_backend(requested.as_deref(), *previous, command_available);
    if requested.as_deref().is_none_or(str::is_empty) {
        *previous = selected.map(|backend| backend.name);
    }
    selected
}

fn select_backend(
    requested: Option<&str>,
    previous: Option<&str>,
    mut is_available: impl FnMut(&str) -> bool,
) -> Option<&'static Backend> {
    if let Some(requested) = requested.filter(|requested| !requested.is_empty()) {
        return BACKENDS
            .iter()
            .find(|backend| backend.name == requested && is_available(backend.argv[0]));
    }
    previous
        .and_then(|name| BACKENDS.iter().find(|backend| backend.name == name))
        .filter(|backend| is_available(backend.argv[0]))
        .or_else(|| BACKENDS.iter().find(|backend| is_available(backend.argv[0])))
}

fn configured_argv(
    backend: &'static Backend,
    quality: Option<u8>,
    max_dimension: Option<u32>,
) -> Vec<String> {
    let quality = quality.map(|quality| quality.clamp(1, 100));
    let dimension = max_dimension.filter(|dimension| *dimension > 0);
    let mut argv = backend.argv.iter().map(|value| (*value).to_string()).collect::<Vec<_>>();

    if matches!(backend.name, "magick" | "convert" | "gm") {
        let output = argv.pop().expect("media backend has an output argument");
        if let Some(quality) = quality {
            argv.extend(["-quality".to_string(), quality.to_string()]);
        }
        if let Some(dimension) = dimension {
            argv.extend(["-resize".to_string(), format!("{dimension}x{dimension}>")]);
        }
        argv.push(output);
    } else {
        let format_index = argv.iter().position(|value| value == "-f").unwrap_or(argv.len());
        let mut options = Vec::new();
        if let Some(quality) = quality {
            options.extend(["-quality".to_string(), quality.to_string()]);
        }
        if let Some(dimension) = dimension {
            options.extend([
                "-vf".to_string(),
                format!(
                    "scale='min(iw,{dimension})':'min(ih,{dimension})':force_original_aspect_ratio=decrease"
                ),
            ]);
        }
        argv.splice(format_index..format_index, options);
    }
    argv
}

pub(crate) fn webp_info(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 30 || &data[..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return None;
    }
    let (width, height) = match &data[12..16] {
        b"VP8X" => (
            u32::from_le_bytes([data[24], data[25], data[26], 0]).saturating_add(1),
            u32::from_le_bytes([data[27], data[28], data[29], 0]).saturating_add(1),
        ),
        b"VP8 " => (
            u16::from_le_bytes([data[26], data[27]]) as u32 & 0x3fff,
            u16::from_le_bytes([data[28], data[29]]) as u32 & 0x3fff,
        ),
        b"VP8L" => {
            let bits = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);
            ((bits & 0x3fff).saturating_add(1), ((bits >> 14) & 0x3fff).saturating_add(1))
        }
        _ => return None,
    };
    (width > 0 && height > 0).then_some((width, height))
}

pub(crate) fn valid_webp(path: &Path) -> bool {
    let mut file = match MediaFile::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut header = [0_u8; 30];
    file.read_exact(&mut header).is_ok() && webp_info(&header).is_some()
}

fn read_stderr_tail(mut reader: impl Read + Send + 'static) -> MediaJoinHandle<io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            output.extend_from_slice(&buffer[..read]);
            if output.len() > STDERR_LIMIT {
                let start = output.len() - STDERR_LIMIT;
                output.drain(..start);
            }
        }
        Ok(output)
    })
}

fn join_stderr(handle: MediaJoinHandle<io::Result<Vec<u8>>>) -> String {
    handle
        .join()
        .ok()
        .and_then(Result::ok)
        .map(|value| String::from_utf8_lossy(&value).trim().to_string())
        .unwrap_or_default()
}

fn terminate(child: &mut MediaChild) {
    let _ = child.kill();
    let _ = child.wait();
}

#[allow(dead_code)]
fn wait_encoder(
    backend_name: &str,
    encoder: &mut MediaChild,
    stderr: MediaJoinHandle<io::Result<Vec<u8>>>,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    let status = loop {
        match encoder.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                terminate(encoder);
                let detail = join_stderr(stderr);
                eprintln!("rngit: media conversion via {backend_name} timed out after {timeout:?}: {detail}");
                return false;
            }
            Err(error) => {
                terminate(encoder);
                let detail = join_stderr(stderr);
                eprintln!("rngit: media conversion via {backend_name} could not wait: {error}: {detail}");
                return false;
            }
        }
    };
    let detail = join_stderr(stderr);
    if !status.success() {
        eprintln!("rngit: media conversion via {backend_name} failed: {detail}");
    }
    status.success()
}

fn wait_pipeline_with_cancel(
    backend_name: &str,
    input: &mut MediaChild,
    encoder: &mut MediaChild,
    input_stderr: MediaJoinHandle<io::Result<Vec<u8>>>,
    encoder_stderr: MediaJoinHandle<io::Result<Vec<u8>>>,
    timeout: Duration,
    mut cancelled: impl FnMut() -> bool,
) -> bool {
    wait_pipeline_with_status(
        backend_name,
        input,
        encoder,
        input_stderr,
        encoder_stderr,
        timeout,
        |child| {
            if cancelled() {
                Err(io::Error::new(io::ErrorKind::Interrupted, "media conversion cancelled"))
            } else {
                child.try_wait()
            }
        },
    )
}

fn wait_pipeline_with_status(
    backend_name: &str,
    input: &mut MediaChild,
    encoder: &mut MediaChild,
    input_stderr: MediaJoinHandle<io::Result<Vec<u8>>>,
    encoder_stderr: MediaJoinHandle<io::Result<Vec<u8>>>,
    timeout: Duration,
    mut try_wait: impl FnMut(&mut MediaChild) -> io::Result<Option<ExitStatus>>,
) -> bool {
    let deadline = Instant::now() + timeout;
    let mut input_status: Option<ExitStatus> = None;
    let mut encoder_status: Option<ExitStatus> = None;
    while input_status.is_none() || encoder_status.is_none() {
        if input_status.is_none() {
            match try_wait(input) {
                Ok(status) => input_status = status,
                Err(error) => {
                    terminate(input);
                    terminate(encoder);
                    let input_detail = join_stderr(input_stderr);
                    let encoder_detail = join_stderr(encoder_stderr);
                    eprintln!(
                        "rngit: could not inspect {backend_name} media pipeline status: {error} (input: {input_detail}; encoder: {encoder_detail})"
                    );
                    return false;
                }
            }
        }
        if encoder_status.is_none() {
            match try_wait(encoder) {
                Ok(status) => encoder_status = status,
                Err(error) => {
                    terminate(input);
                    terminate(encoder);
                    let input_detail = join_stderr(input_stderr);
                    let encoder_detail = join_stderr(encoder_stderr);
                    eprintln!(
                        "rngit: could not inspect {backend_name} media pipeline status: {error} (input: {input_detail}; encoder: {encoder_detail})"
                    );
                    return false;
                }
            }
        }
        if input_status.is_some() && encoder_status.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            terminate(input);
            terminate(encoder);
            let input_detail = join_stderr(input_stderr);
            let encoder_detail = join_stderr(encoder_stderr);
            eprintln!(
                "rngit: media conversion via {backend_name} timed out after {timeout:?} (input: {input_detail}; encoder: {encoder_detail})"
            );
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let input_detail = join_stderr(input_stderr);
    let encoder_detail = join_stderr(encoder_stderr);
    let input_ok = input_status.is_some_and(|status| status.success());
    let encoder_ok = encoder_status.is_some_and(|status| status.success());
    if !input_ok || !encoder_ok {
        eprintln!(
            "rngit: media conversion via {backend_name} failed (input: {input_detail}; encoder: {encoder_detail})"
        );
    }
    input_ok && encoder_ok
}

fn spawn_command(argv: &[String], cwd: Option<&Path>) -> io::Result<MediaChild> {
    let Some((program, args)) = argv.split_first() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty media command"));
    };
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.spawn()
}

pub(crate) fn convert_to_webp_with_cancel(
    input_argv: &[String],
    output_path: &Path,
    cwd: Option<&Path>,
    timeout: Option<Duration>,
    quality: Option<u8>,
    max_dimension: Option<u32>,
    cancelled: impl FnMut() -> bool,
) -> bool {
    let Some(backend) = selected_backend() else { return false };
    let encoder_argv = configured_argv(backend, quality, max_dimension);
    let mut input = match spawn_command(input_argv, cwd) {
        Ok(child) => child,
        Err(error) => {
            eprintln!("rngit: could not start media input: {error}");
            return false;
        }
    };
    let Some(input_stdout) = input.stdout.take() else {
        terminate(&mut input);
        return false;
    };
    let input_stderr = input.stderr.take().map(read_stderr_tail);
    let output = match MediaFile::create(output_path) {
        Ok(output) => output,
        Err(error) => {
            terminate(&mut input);
            if let Some(stderr) = input_stderr {
                let _ = join_stderr(stderr);
            }
            eprintln!("rngit: could not create media output {}: {error}", output_path.display());
            return false;
        }
    };
    let mut encoder_command = Command::new(&encoder_argv[0]);
    encoder_command
        .args(&encoder_argv[1..])
        .stdin(input_stdout)
        .stdout(output)
        .stderr(Stdio::piped());
    let mut encoder = match encoder_command.spawn() {
        Ok(child) => child,
        Err(error) => {
            terminate(&mut input);
            if let Some(stderr) = input_stderr {
                let _ = join_stderr(stderr);
            }
            eprintln!("rngit: could not start {} media backend: {error}", backend.name);
            return false;
        }
    };
    let Some(input_stderr) = input_stderr else {
        terminate(&mut input);
        terminate(&mut encoder);
        return false;
    };
    let Some(encoder_stderr) = encoder.stderr.take().map(read_stderr_tail) else {
        terminate(&mut input);
        terminate(&mut encoder);
        let _ = join_stderr(input_stderr);
        return false;
    };
    let ok = wait_pipeline_with_cancel(
        backend.name,
        &mut input,
        &mut encoder,
        input_stderr,
        encoder_stderr,
        timeout.unwrap_or(MEDIA_CONVERSION_TIMEOUT),
        cancelled,
    );
    if !ok || !valid_webp(output_path) {
        let _ = fs::remove_file(output_path);
        return false;
    }
    true
}

#[allow(dead_code)]
pub(crate) fn convert_file_to_webp(
    source_path: &Path,
    quality: Option<u8>,
    max_dimension: Option<u32>,
    timeout: Option<Duration>,
) -> Option<PathBuf> {
    let backend = selected_backend()?;
    let encoder_argv = configured_argv(backend, quality, max_dimension);
    let input = MediaFile::open(source_path).ok()?;
    let temporary = tempfile::Builder::new()
        .prefix("rngit-media-")
        .suffix(".webp")
        .tempfile()
        .ok()?
        .into_temp_path();
    let output_path = temporary.to_path_buf();
    let output = MediaFile::create(&output_path).ok()?;
    let mut encoder_command = Command::new(&encoder_argv[0]);
    encoder_command
        .args(&encoder_argv[1..])
        .stdin(input)
        .stdout(output)
        .stderr(Stdio::piped());
    let mut encoder = encoder_command.spawn().ok()?;
    let Some(stderr) = encoder.stderr.take().map(read_stderr_tail) else {
        terminate(&mut encoder);
        return None;
    };
    if !wait_encoder(
        backend.name,
        &mut encoder,
        stderr,
        timeout.unwrap_or(MEDIA_CONVERSION_TIMEOUT),
    ) || !valid_webp(&output_path)
    {
        let _ = fs::remove_file(&output_path);
        return None;
    }
    temporary.keep().ok()
}

#[cfg(test)]
#[path = "media_backend_tests.rs"]
mod media_backend_tests;

#[cfg(all(test, unix))]
#[path = "issue_613_converter_process_tests.rs"]
mod issue_613_converter_process_tests;
