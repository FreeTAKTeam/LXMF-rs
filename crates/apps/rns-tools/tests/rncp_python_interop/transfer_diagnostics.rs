use super::{
    read_resource_advertisement_observations, spawn_python_listener_with_save_root,
    PythonListenerOptions, PythonRuntime,
};
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Output};
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub(super) fn spawn_python_listener_capturing_stdout(
    runtime: &PythonRuntime<'_>,
    config_dir: &Path,
    identity: &Path,
    root: &Path,
    allowed_identities: &[&str],
    no_compress: bool,
) -> io::Result<Child> {
    spawn_python_listener_with_save_root(
        runtime,
        PythonListenerOptions {
            config_dir,
            identity,
            jail_root: root,
            save_root: root,
            allowed_identities,
            no_compress,
            announce_interval_seconds: 0,
            verbose: false,
            capture_stdout: true,
        },
    )
}

pub(super) fn capture_listener_logs(child: &mut Child) -> io::Result<Receiver<String>> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Python rncp listener stdout was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Python rncp listener stderr was not captured"))?;
    let (log_tx, log_rx) = mpsc::channel();
    for (stream_name, stream) in [
        ("stdout", Box::new(stdout) as Box<dyn Read + Send>),
        ("stderr", Box::new(stderr) as Box<dyn Read + Send>),
    ] {
        let stream_tx = log_tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                if stream_tx.send(format!("{stream_name}: {line}")).is_err() {
                    break;
                }
            }
        });
    }
    drop(log_tx);
    Ok(log_rx)
}

pub(super) fn no_compression_fetch_failure(
    fetched: &Output,
    listener_logs: &Receiver<String>,
    resource_observation_log: &Path,
    temporary_root: &Path,
    phase_trace: &[String],
) -> io::Error {
    let advertisements = read_resource_advertisement_observations(resource_observation_log)
        .map(|observations| format!("{observations:?}"))
        .unwrap_or_else(|error| format!("unavailable: {error}"));
    let logs = redact_transfer_diagnostics(
        &listener_logs.try_iter().collect::<Vec<_>>().join("\n"),
        temporary_root,
    );
    let mut phases = phase_trace.to_vec();
    phases.push(format!("event=rust_fetch_exited status={}", fetched.status));
    io::Error::other(format!(
        "Rust rncp no-compression fetch failed: {}\nstdout:\n{}\nstderr:\n{}\nPython listener logs:\n{}\nResource advertisement observations: {}\nPhase trace:\n{}",
        fetched.status,
        redact_transfer_diagnostics(&String::from_utf8_lossy(&fetched.stdout), temporary_root),
        redact_transfer_diagnostics(&String::from_utf8_lossy(&fetched.stderr), temporary_root),
        logs,
        advertisements,
        phases.join("\n")
    ))
}

fn redact_transfer_diagnostics(text: &str, temporary_root: &Path) -> String {
    let mut redacted = text.replace(&temporary_root.display().to_string(), "<temp>");
    for token in text.split_whitespace() {
        let hash = token.trim_matches(|character: char| !character.is_ascii_hexdigit());
        if hash.len() >= 32 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            redacted = redacted.replace(hash, "<redacted-hash>");
        }
    }
    redacted
}
