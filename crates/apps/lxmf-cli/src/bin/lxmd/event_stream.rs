//! Push-based `--on-inbound` delivery over the daemon's `/events/stream`.
//!
//! Polling `sdk_poll_events_v2` added up to the idle sleep (250 ms) to every
//! hook run; the stream hands each event over as the daemon publishes it.

use crate::rpc_client::{decode_rpc_frame, resolve_socket_addr};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const RECONNECT_DELAY: Duration = Duration::from_millis(250);
/// Matches the daemon codec's frame payload limit.
const MAX_FRAME_PAYLOAD_LEN: usize = 16 * 1024 * 1024;

pub(crate) fn spawn_on_inbound_watcher(
    rpc_addr: String,
    command: String,
    messages_dir: Option<PathBuf>,
) {
    thread::spawn(move || {
        let mut cursor: Option<String> = None;
        loop {
            let resume = cursor.clone();
            let result = stream_events(&rpc_addr, resume.as_deref(), |event| {
                if let Some(next) = cursor_after(event) {
                    cursor = Some(next);
                }
                if let Err(err) =
                    crate::inbound::run_on_inbound_command(&command, event, messages_dir.as_deref())
                {
                    log::error!("on-inbound hook failed: {err}");
                }
            });
            match result {
                // The daemon closed the stream (for example a catch-up error); resume.
                Ok(()) => {}
                Err(StreamError::Cursor(err)) => {
                    log::warn!("inbound event stream cursor reset: {err}");
                    cursor = None;
                }
                Err(StreamError::Fatal(err)) => {
                    log::error!("inbound event watcher stopped: {err}");
                    break;
                }
            }
            thread::sleep(RECONNECT_DELAY);
        }
    });
}

#[derive(Debug)]
pub(crate) enum StreamError {
    /// The resume cursor is unusable; reconnect from the retained window.
    Cursor(String),
    Fatal(String),
}

/// Reads stream frames until the daemon closes the connection.
pub(crate) fn stream_events(
    rpc_addr: &str,
    cursor: Option<&str>,
    mut on_event: impl FnMut(&serde_json::Value),
) -> Result<(), StreamError> {
    let fatal = |err: String| StreamError::Fatal(err);
    let addr = resolve_socket_addr(rpc_addr).map_err(fatal)?;
    let mut stream =
        TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|err| fatal(err.to_string()))?;
    stream.set_read_timeout(Some(CONNECT_TIMEOUT)).map_err(|err| fatal(err.to_string()))?;
    let path = match cursor {
        Some(cursor) => format!("/events/stream?cursor={cursor}"),
        None => "/events/stream".to_string(),
    };
    let request = format!("GET {path} HTTP/1.1\r\nHost: {rpc_addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|err| fatal(err.to_string()))?;

    let mut reader = BufReader::new(stream);
    let status = read_status(&mut reader).map_err(fatal)?;
    if status != 200 {
        let mut body = Vec::new();
        if let Err(err) = reader.read_to_end(&mut body) {
            return Err(fatal(format!("event stream rejected with HTTP {status}: {err}")));
        }
        return Err(decode_rpc_frame(&body)
            .ok()
            .and_then(|value| response_error(&value))
            .unwrap_or_else(|| fatal(format!("event stream rejected with HTTP {status}"))));
    }
    // Events arrive whenever the daemon publishes them; idle periods are normal.
    reader.get_ref().set_read_timeout(None).map_err(|err| fatal(err.to_string()))?;

    loop {
        let mut len = [0u8; 4];
        match reader.read_exact(&mut len) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(err) => return Err(fatal(format!("event stream read failed: {err}"))),
        }
        let payload_len = u32::from_be_bytes(len) as usize;
        if payload_len > MAX_FRAME_PAYLOAD_LEN {
            return Err(fatal(format!("event stream frame of {payload_len} bytes is too large")));
        }
        let mut frame = len.to_vec();
        frame.resize(4 + payload_len, 0);
        reader
            .read_exact(&mut frame[4..])
            .map_err(|err| fatal(format!("event stream frame truncated: {err}")))?;
        let value = decode_rpc_frame(&frame).map_err(fatal)?;
        if let Some(err) = response_error(&value) {
            return Err(err);
        }
        on_event(&value);
    }
}

fn read_status(reader: &mut impl BufRead) -> Result<u16, String> {
    let mut status_line = String::new();
    reader.read_line(&mut status_line).map_err(|err| format!("event stream header: {err}"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("invalid event stream status line {status_line:?}"))?;
    loop {
        let mut line = String::new();
        let read =
            reader.read_line(&mut line).map_err(|err| format!("event stream header: {err}"))?;
        if read == 0 || line == "\r\n" {
            return Ok(status);
        }
    }
}

/// Error frames are RPC responses; event frames are event maps.
fn response_error(value: &serde_json::Value) -> Option<StreamError> {
    let error = value.get("error")?;
    let code = error.get("code").and_then(|code| code.as_str()).unwrap_or("RPC_ERROR");
    let message = error.get("message").and_then(|message| message.as_str()).unwrap_or("");
    let text = format!("{code}: {message}");
    Some(
        if matches!(
            code,
            "SDK_RUNTIME_CURSOR_EXPIRED"
                | "SDK_RUNTIME_STREAM_DEGRADED"
                | "SDK_RUNTIME_INVALID_CURSOR"
        ) {
            StreamError::Cursor(text)
        } else {
            StreamError::Fatal(text)
        },
    )
}

/// Resume cursor after `event`, in the daemon's `v2:runtime:stream:seq` form.
fn cursor_after(event: &serde_json::Value) -> Option<String> {
    let runtime = event.get("runtime_id")?.as_str()?;
    let stream = event.get("stream_id")?.as_str()?;
    let seq_no = event.get("seq_no")?.as_u64()?;
    Some(format!("v2:{runtime}:{stream}:{seq_no}"))
}

#[cfg(test)]
#[path = "event_stream_tests.rs"]
mod tests;
