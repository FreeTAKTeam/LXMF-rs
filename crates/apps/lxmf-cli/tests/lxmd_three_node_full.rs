use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TEST_TIMEOUT: Duration = Duration::from_secs(300);

#[test]
fn lxmd_three_node_tcp_full_test_waits_for_announces_before_delivery() {
    let lxmd_bin = resolve_test_binary("lxmd", option_env!("CARGO_BIN_EXE_lxmd"));
    let reticulumd_bin = resolve_test_binary("reticulumd", option_env!("CARGO_BIN_EXE_reticulumd"));

    let temp = tempfile::tempdir().expect("tempdir");
    let server_port = reserve_local_port();
    let server_rpc = reserve_local_port();
    let client_two_rpc = reserve_local_port();
    let client_three_rpc = reserve_local_port();

    let server_dir = temp.path().join("server");
    let client_two_dir = temp.path().join("client-two");
    let client_three_dir = temp.path().join("client-three");

    write_config(&server_dir, &node_config("server", server_rpc, "tcp_server", server_port));
    write_config(
        &client_two_dir,
        &node_config("client-two", client_two_rpc, "tcp_client", server_port),
    );
    write_config(
        &client_three_dir,
        &node_config("client-three", client_three_rpc, "tcp_client", server_port),
    );

    let mut server = spawn_lxmd(&lxmd_bin, &reticulumd_bin, &server_dir);
    let mut client_two = spawn_lxmd(&lxmd_bin, &reticulumd_bin, &client_two_dir);
    let mut client_three = spawn_lxmd(&lxmd_bin, &reticulumd_bin, &client_three_dir);

    let outcome = (|| {
        wait_for_ready(server_rpc, &mut server, "server")?;
        wait_for_ready(client_two_rpc, &mut client_two, "client-two")?;
        wait_for_ready(client_three_rpc, &mut client_three, "client-three")?;

        let client_two_hash = daemon_status(client_two_rpc)?["delivery_destination_hash"]
            .as_str()
            .expect("client-two delivery hash")
            .to_string();
        let client_three_hash = daemon_status(client_three_rpc)?["delivery_destination_hash"]
            .as_str()
            .expect("client-three delivery hash")
            .to_string();

        rpc_call(client_two_rpc, "announce_now", None)?;
        rpc_call(client_three_rpc, "announce_now", None)?;

        wait_for_announce(client_two_rpc, &client_three_hash)?;
        wait_for_announce(client_three_rpc, &client_two_hash)?;

        let message_id = format!(
            "hello-world-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_millis()
        );
        rpc_call(
            client_three_rpc,
            "send_message_v2",
            Some(json!({
                "id": message_id,
                "source": client_three_hash,
                "destination": client_two_hash,
                "title": "",
                "content": "hello world",
                "method": "direct"
            })),
        )?;

        wait_for_inbound_message(client_two_rpc, "hello world")
    })();

    terminate_child(&mut client_three);
    terminate_child(&mut client_two);
    terminate_child(&mut server);

    if let Err(err) = outcome {
        panic!("three-node lxmd flow failed: {err}");
    }
}

fn resolve_test_binary_if_present(name: &str, provided: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = provided.filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }

    if let Some(path) = std::env::var_os(format!("{}_BIN", name.to_ascii_uppercase()))
        .filter(|path| !path.is_empty())
    {
        return Some(PathBuf::from(path));
    }

    let current_exe = std::env::current_exe().expect("current test executable path");
    let deps_dir = current_exe.parent().expect("test executable parent");
    let target_dir = deps_dir.parent().expect("target debug dir");
    let candidate = target_dir.join(name);
    candidate.exists().then_some(candidate)
}

fn resolve_test_binary(name: &str, provided: Option<&str>) -> PathBuf {
    if let Some(path) = resolve_test_binary_if_present(name, provided) {
        return path;
    }

    panic!("failed to locate {name} test binary via CARGO_BIN_EXE or target/debug fallback");
}

fn node_config(name: &str, rpc_port: u16, interface_type: &str, server_port: u16) -> String {
    let interface = match interface_type {
        "tcp_server" => format!(
            "[[interfaces]]\ntype = \"tcp_server\"\nenabled = true\nname = \"{name}-server\"\nhost = \"0.0.0.0\"\nport = {server_port}\n"
        ),
        "tcp_client" => format!(
            "[[interfaces]]\ntype = \"tcp_client\"\nenabled = true\nname = \"{name}-uplink\"\nhost = \"127.0.0.1\"\nport = {server_port}\n"
        ),
        other => panic!("unsupported interface type {other}"),
    };

    format!(
        r#"[node]
display_name = "{name}"

[rpc]
listen = "127.0.0.1:{rpc_port}"

[storage]
db = "./state/reticulum.db"
identity = "./state/identity"

[lxmf]
announce_at_start = false

{interface}"#
    )
}

fn write_config(dir: &Path, config: &str) {
    fs::create_dir_all(dir.join("state")).expect("create state dir");
    fs::write(dir.join("lxmd.toml"), config).expect("write config");
}

fn spawn_lxmd(lxmd_bin: &Path, reticulumd_bin: &Path, config_dir: &Path) -> Child {
    Command::new(lxmd_bin)
        .arg("--config")
        .arg(config_dir.join("lxmd.toml"))
        .env("RETICULUMD_BIN", reticulumd_bin)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn lxmd")
}

fn wait_for_ready(rpc_port: u16, child: &mut Child, label: &str) -> Result<(), String> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            let stderr = child.stderr.take().map(read_to_string).unwrap_or_default();
            return Err(format!("{label} exited early with {status}: {stderr}"));
        }
        match http_get_ready(rpc_port) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(_) => {}
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err(format!("timed out waiting for {label} readyz on port {rpc_port}"))
}

fn daemon_status(rpc_port: u16) -> Result<Value, String> {
    rpc_call(rpc_port, "daemon_status_ex", None)
}

fn wait_for_announce(rpc_port: u16, expected_destination: &str) -> Result<(), String> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    while Instant::now() < deadline {
        let announces = rpc_call(rpc_port, "list_announces", Some(json!({ "limit": 200 })))?;
        let seen = announces["announces"].as_array().is_some_and(|items| {
            items.iter().any(|announce| {
                announce["destination"].as_str() == Some(expected_destination)
                    || announce["source"].as_str() == Some(expected_destination)
                    || announce["peer"].as_str() == Some(expected_destination)
            })
        });
        if seen {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(format!("announce for {expected_destination} was not discovered on rpc port {rpc_port}"))
}

fn wait_for_inbound_message(rpc_port: u16, expected_content: &str) -> Result<(), String> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    while Instant::now() < deadline {
        let messages = rpc_call(rpc_port, "list_messages", None)?;
        let delivered = messages["messages"].as_array().is_some_and(|items| {
            items.iter().any(|message| {
                message["direction"].as_str() == Some("in")
                    && message["content"].as_str() == Some(expected_content)
            })
        });
        if delivered {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(format!("inbound content '{expected_content}' did not arrive on rpc port {rpc_port}"))
}

fn rpc_call(rpc_port: u16, method: &str, params: Option<Value>) -> Result<Value, String> {
    let payload = encode_rpc_frame(json!({
        "id": 1,
        "method": method,
        "params": params,
    }))?;
    let request = format!(
        "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{rpc_port}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let mut bytes = request.into_bytes();
    bytes.extend_from_slice(&payload);
    let response = http_request(rpc_port, &bytes)?;
    let body = http_body(&response).ok_or_else(|| "missing rpc response body".to_string())?;
    let value: Value = decode_rpc_frame(body)?;
    if let Some(error) = value.get("error") {
        if !error.is_null() {
            return Err(error.to_string());
        }
    }
    value.get("result").cloned().ok_or_else(|| format!("rpc response for {method} missing result"))
}

fn http_get_ready(rpc_port: u16) -> Result<bool, String> {
    let response = http_request(
        rpc_port,
        format!("GET /readyz HTTP/1.1\r\nHost: 127.0.0.1:{rpc_port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )?;
    let body = http_body(&response).ok_or_else(|| "missing readyz body".to_string())?;
    Ok(body == b"ok")
}

fn http_request(rpc_port: u16, request: &[u8]) -> Result<Vec<u8>, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", rpc_port)).map_err(|err| err.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|err| err.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|err| err.to_string())?;
    stream.write_all(request).map_err(|err| err.to_string())?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|err| err.to_string())?;
    Ok(response)
}

fn http_body(response: &[u8]) -> Option<&[u8]> {
    response.windows(4).position(|window| window == b"\r\n\r\n").map(|index| &response[index + 4..])
}

fn encode_rpc_frame(value: Value) -> Result<Vec<u8>, String> {
    let payload = rmp_serde::to_vec(&value).map_err(|err| err.to_string())?;
    let len = u32::try_from(payload.len()).map_err(|_| "rpc frame too large".to_string())?;
    let mut bytes = len.to_be_bytes().to_vec();
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

fn decode_rpc_frame(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() < 4 {
        return Err("rpc response too short".to_string());
    }
    let frame_len = u32::from_be_bytes(bytes[..4].try_into().expect("frame header")) as usize;
    if bytes.len() < 4 + frame_len {
        return Err("rpc response incomplete".to_string());
    }
    rmp_serde::from_slice(&bytes[4..4 + frame_len]).map_err(|err| err.to_string())
}

fn terminate_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn read_to_string(mut reader: impl Read) -> String {
    let mut output = String::new();
    let _ = reader.read_to_string(&mut output);
    output
}

fn reserve_local_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind ephemeral port")
        .local_addr()
        .expect("ephemeral local addr")
        .port()
}
