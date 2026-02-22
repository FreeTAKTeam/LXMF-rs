use crate::cli::app::{DaemonAction, DaemonCommand, RuntimeContext};
use crate::cli::daemon::{DaemonStatus, DaemonSupervisor};
use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
struct DaemonProbeReport {
    profile: String,
    local: DaemonStatus,
    rpc: RpcProbeReport,
    events: EventsProbeReport,
}

#[derive(Debug, Clone, Serialize)]
struct RpcProbeReport {
    reachable: bool,
    endpoint: String,
    method: Option<String>,
    roundtrip_ms: Option<u128>,
    identity_hash: Option<String>,
    status: Option<serde_json::Value>,
    errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct EventsProbeReport {
    reachable: bool,
    endpoint: String,
    roundtrip_ms: Option<u128>,
    event_type: Option<String>,
    payload: Option<serde_json::Value>,
    error: Option<String>,
}

pub fn run(ctx: &RuntimeContext, command: &DaemonCommand) -> Result<()> {
    let supervisor = DaemonSupervisor::new(&ctx.profile_name, ctx.profile_settings.clone());

    match &command.action {
        DaemonAction::Start { managed, reticulumd, transport } => {
            let managed_override = (*managed).then_some(true);
            let status =
                supervisor.start(reticulumd.clone(), managed_override, transport.clone())?;
            ctx.output.emit_status(&status)
        }
        DaemonAction::Stop => {
            let status = supervisor.stop()?;
            ctx.output.emit_status(&status)
        }
        DaemonAction::Restart { managed, reticulumd, transport } => {
            let managed_override = (*managed).then_some(true);
            let status =
                supervisor.restart(reticulumd.clone(), managed_override, transport.clone())?;
            ctx.output.emit_status(&status)
        }
        DaemonAction::Status => {
            let local = supervisor.status()?;
            let rpc_status = ctx.rpc.call("daemon_status_ex", None).ok();
            ctx.output.emit_status(&json!({
                "profile": ctx.profile_name,
                "local": local,
                "rpc": rpc_status,
            }))
        }
        DaemonAction::Probe => {
            let local = supervisor.status()?;
            let rpc_probe = probe_rpc_status(ctx);
            let events_probe = probe_events(ctx);
            ctx.output.emit_status(&DaemonProbeReport {
                profile: ctx.profile_name.clone(),
                local,
                rpc: rpc_probe,
                events: events_probe,
            })
        }
        DaemonAction::Logs { tail } => {
            let lines = supervisor.logs(*tail)?;
            ctx.output.emit_lines(&lines);
            Ok(())
        }
    }
}

fn probe_rpc_status(ctx: &RuntimeContext) -> RpcProbeReport {
    let started = Instant::now();
    let mut failures = Vec::new();
    for method in ["daemon_status_ex", "status"] {
        match ctx.rpc.call(method, None) {
            Ok(status) => {
                let identity_hash = extract_identity_hash(&status);
                return RpcProbeReport {
                    reachable: true,
                    endpoint: ctx.profile_settings.rpc.clone(),
                    method: Some(method.to_string()),
                    roundtrip_ms: Some(started.elapsed().as_millis()),
                    identity_hash,
                    status: Some(status),
                    errors: failures,
                };
            }
            Err(err) => failures.push(format!("{method}: {err}")),
        }
    }

    RpcProbeReport {
        reachable: false,
        endpoint: ctx.profile_settings.rpc.clone(),
        method: None,
        roundtrip_ms: None,
        identity_hash: None,
        status: None,
        errors: failures,
    }
}

fn probe_events(ctx: &RuntimeContext) -> EventsProbeReport {
    let started = Instant::now();
    match ctx.rpc.poll_event() {
        Ok(Some(event)) => EventsProbeReport {
            reachable: true,
            endpoint: ctx.profile_settings.rpc.clone(),
            roundtrip_ms: Some(started.elapsed().as_millis()),
            event_type: Some(event.event_type),
            payload: Some(event.payload),
            error: None,
        },
        Ok(None) => EventsProbeReport {
            reachable: true,
            endpoint: ctx.profile_settings.rpc.clone(),
            roundtrip_ms: Some(started.elapsed().as_millis()),
            event_type: None,
            payload: None,
            error: None,
        },
        Err(err) => EventsProbeReport {
            reachable: false,
            endpoint: ctx.profile_settings.rpc.clone(),
            roundtrip_ms: None,
            event_type: None,
            payload: None,
            error: Some(err.to_string()),
        },
    }
}

fn extract_identity_hash(status: &serde_json::Value) -> Option<String> {
    for key in ["delivery_destination_hash", "identity_hash"] {
        if let Some(hash) = status
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|candidate| !candidate.is_empty())
        {
            return Some(hash.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{extract_identity_hash, probe_events, probe_rpc_status};
    use crate::cli::app::{Cli, Command, DaemonAction, DaemonCommand, RuntimeContext};
    use crate::cli::output::Output;
    use crate::cli::profile::{init_profile, load_profile_settings, profile_paths};
    use crate::cli::rpc_client::RpcClient;
    use serde::Serialize;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Mutex, OnceLock};
    use std::thread;

    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn extract_identity_hash_prefers_delivery_hash() {
        let status = json!({
            "identity_hash": "identity-hash",
            "delivery_destination_hash": "delivery-hash"
        });
        assert_eq!(extract_identity_hash(&status), Some("delivery-hash".into()));
    }

    #[test]
    fn probe_events_marks_unreachable_on_http_error() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
        init_profile("daemon-probe-events", false, None).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let response =
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            stream.write_all(response).unwrap();
            stream.flush().unwrap();
        });

        let mut settings = load_profile_settings("daemon-probe-events").unwrap();
        settings.rpc = format!("127.0.0.1:{}", addr.port());
        let ctx = RuntimeContext {
            cli: Cli {
                profile: "daemon-probe-events".into(),
                rpc: None,
                json: true,
                quiet: true,
                command: Command::Daemon(DaemonCommand { action: DaemonAction::Probe }),
            },
            profile_name: "daemon-probe-events".into(),
            profile_settings: settings.clone(),
            profile_paths: profile_paths("daemon-probe-events").unwrap(),
            rpc: RpcClient::new(&settings.rpc),
            output: Output::new(true, true),
        };

        let probe = probe_events(&ctx);
        assert!(!probe.reachable);
        assert!(probe.error.as_deref().unwrap_or_default().contains("event poll failed"));

        worker.join().unwrap();
        std::env::remove_var("LXMF_CONFIG_ROOT");
    }

    #[test]
    fn probe_rpc_status_falls_back_to_status_method() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
        init_profile("daemon-probe-rpc", false, None).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            for idx in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_http_request(&mut stream);
                assert_eq!(request.http_method, "POST");
                assert_eq!(request.path, "/rpc");
                let rpc: RpcRequest = decode_frame(&request.body);
                assert!(rpc.params.is_none());
                match idx {
                    0 => {
                        assert_eq!(rpc.method, "daemon_status_ex");
                        let response = RpcResponse {
                            id: rpc.id,
                            result: None,
                            error: Some(RpcError {
                                code: "unavailable".into(),
                                message: "daemon status not ready".into(),
                            }),
                        };
                        write_http_response(&mut stream, 200, &encode_frame(&response));
                    }
                    _ => {
                        assert_eq!(rpc.method, "status");
                        let response = RpcResponse {
                            id: rpc.id,
                            result: Some(json!({
                                "identity_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                            })),
                            error: None,
                        };
                        write_http_response(&mut stream, 200, &encode_frame(&response));
                    }
                }
            }
        });

        let mut settings = load_profile_settings("daemon-probe-rpc").unwrap();
        settings.rpc = format!("127.0.0.1:{}", addr.port());
        let ctx = RuntimeContext {
            cli: Cli {
                profile: "daemon-probe-rpc".into(),
                rpc: None,
                json: true,
                quiet: true,
                command: Command::Daemon(DaemonCommand { action: DaemonAction::Probe }),
            },
            profile_name: "daemon-probe-rpc".into(),
            profile_settings: settings.clone(),
            profile_paths: profile_paths("daemon-probe-rpc").unwrap(),
            rpc: RpcClient::new(&settings.rpc),
            output: Output::new(true, true),
        };

        let probe = probe_rpc_status(&ctx);
        assert!(probe.reachable);
        assert_eq!(probe.method.as_deref(), Some("status"));
        assert_eq!(probe.identity_hash.as_deref(), Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert_eq!(probe.errors.len(), 1);
        assert!(probe.errors[0].contains("daemon_status_ex"));

        worker.join().unwrap();
        std::env::remove_var("LXMF_CONFIG_ROOT");
    }

    #[derive(Debug, Serialize)]
    struct RpcResponse {
        id: u64,
        result: Option<serde_json::Value>,
        error: Option<RpcError>,
    }

    #[derive(Debug, Serialize)]
    struct RpcError {
        code: String,
        message: String,
    }

    #[derive(Debug, serde::Deserialize)]
    struct RpcRequest {
        id: u64,
        method: String,
        params: Option<serde_json::Value>,
    }

    struct HttpRequest {
        http_method: String,
        path: String,
        body: Vec<u8>,
    }

    fn read_http_request(stream: &mut TcpStream) -> HttpRequest {
        let mut bytes = Vec::new();
        let mut header_end = None;
        let mut content_length = 0usize;

        loop {
            let mut buf = [0u8; 1024];
            let read = stream.read(&mut buf).unwrap();
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buf[..read]);

            if header_end.is_none() {
                if let Some(pos) = find_header_end(&bytes) {
                    header_end = Some(pos);
                    let headers = String::from_utf8_lossy(&bytes[..pos]);
                    content_length = parse_content_length(&headers);
                }
            }

            if let Some(pos) = header_end {
                let body_start = pos + 4;
                if bytes.len() >= body_start + content_length {
                    break;
                }
            }
        }

        let header_end = header_end.expect("valid http request headers");
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let mut lines = headers.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut parts = request_line.split_whitespace();
        let http_method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        let body_start = header_end + 4;
        let body =
            if body_start <= bytes.len() { bytes[body_start..].to_vec() } else { Vec::new() };

        HttpRequest { http_method, path, body }
    }

    fn write_http_response(stream: &mut TcpStream, status_code: u16, body: &[u8]) {
        let status_text = match status_code {
            200 => "OK",
            204 => "No Content",
            _ => "Error",
        };
        let header = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            status_code,
            status_text,
            body.len()
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
        stream.flush().unwrap();
    }

    fn find_header_end(bytes: &[u8]) -> Option<usize> {
        bytes.windows(4).position(|w| w == b"\r\n\r\n")
    }

    fn parse_content_length(headers: &str) -> usize {
        headers
            .lines()
            .find_map(|line| {
                let lower = line.to_ascii_lowercase();
                lower
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0)
    }

    fn encode_frame<T: Serialize>(value: &T) -> Vec<u8> {
        let payload = rmp_serde::to_vec(value).unwrap();
        let mut framed = Vec::with_capacity(payload.len() + 4);
        framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        framed.extend_from_slice(&payload);
        framed
    }

    fn decode_frame<T: for<'de> serde::Deserialize<'de>>(framed: &[u8]) -> T {
        assert!(framed.len() >= 4, "missing frame length");
        let mut len_buf = [0u8; 4];
        len_buf.copy_from_slice(&framed[..4]);
        let len = u32::from_be_bytes(len_buf) as usize;
        assert!(framed.len() >= 4 + len, "incomplete frame");
        rmp_serde::from_slice(&framed[4..4 + len]).unwrap()
    }
}
