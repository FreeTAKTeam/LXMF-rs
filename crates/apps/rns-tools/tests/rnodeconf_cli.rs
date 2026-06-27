use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::process::Command;
use std::thread;

use rns_rpc::rpc::codec;
use rns_rpc::RpcResponse;
use serde_json::{json, Value as JsonValue};

#[test]
fn rnodeconf_sends_query_radio_state_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("radio_state_query"));
        assert!(params.get("pattern").is_none());
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "radio_state_query"
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("query-radio-state")
        .arg("--interface")
        .arg("rnode-main")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["queued"].as_bool(), Some(true));
    assert_eq!(value["command"].as_str(), Some("radio_state_query"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_blink_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("blink"));
        assert_eq!(params["pattern"].as_u64(), Some(3));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "blink",
                "pattern": 3
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("blink")
        .arg("--interface")
        .arg("rnode-main")
        .arg("--pattern")
        .arg("3")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["queued"].as_bool(), Some(true));
    assert_eq!(value["command"].as_str(), Some("blink"));
    assert_eq!(value["pattern"].as_u64(), Some(3));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_read_config_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("config_read"));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "config_read"
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("read-config")
        .arg("--interface")
        .arg("rnode-main")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["command"].as_str(), Some("config_read"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_display_intensity_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("display_intensity"));
        assert_eq!(params["intensity"].as_u64(), Some(8));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "display_intensity",
                "intensity": 8
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("set-display-intensity")
        .arg("--interface")
        .arg("rnode-main")
        .arg("--intensity")
        .arg("8")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["command"].as_str(), Some("display_intensity"));
    assert_eq!(value["intensity"].as_u64(), Some(8));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_disable_interference_avoidance_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("disable_interference_avoidance"));
        assert_eq!(params["disabled"].as_bool(), Some(true));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "disable_interference_avoidance",
                "disabled": true
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("disable-interference-avoidance")
        .arg("--interface")
        .arg("rnode-main")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["command"].as_str(), Some("disable_interference_avoidance"));
    assert_eq!(value["disabled"].as_bool(), Some(true));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_persistent_rnode_management_guard_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("wifi_psk"));
        assert_eq!(params["confirm_persistent"].as_bool(), Some(true));
        assert_eq!(params["psk"].as_str(), Some("abcdefgh"));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "wifi_psk",
                "confirmation": "persistent",
                "psk_set": true
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("set-wifi-psk")
        .arg("--interface")
        .arg("rnode-main")
        .arg("--psk")
        .arg("abcdefgh")
        .arg("--confirm-persistent")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["command"].as_str(), Some("wifi_psk"));
    assert_eq!(value["psk_set"].as_bool(), Some(true));
    assert!(value.get("psk").is_none());
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_destructive_rnode_management_guard_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("rom_write"));
        assert_eq!(params["address"].as_u64(), Some(9));
        assert_eq!(params["byte"].as_u64(), Some(42));
        assert_eq!(params["confirm_destructive"].as_bool(), Some(true));
        assert_eq!(params["confirm_command"].as_str(), Some("rom_write"));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "rom_write",
                "confirmation": "destructive",
                "address": 9,
                "byte": 42
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("write-rom")
        .arg("--interface")
        .arg("rnode-main")
        .arg("--address")
        .arg("9")
        .arg("--byte")
        .arg("42")
        .arg("--confirm-destructive")
        .arg("--confirm-command")
        .arg("rom_write")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["command"].as_str(), Some("rom_write"));
    assert_eq!(value["confirmation"].as_str(), Some("destructive"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_guarded_rnode_multi_vport_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("config_save"));
        assert_eq!(params["vport"].as_u64(), Some(2));
        assert_eq!(params["confirm_persistent"].as_bool(), Some(true));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "config_save",
                "vport": 2,
                "confirmation": "persistent"
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("save-config")
        .arg("--interface")
        .arg("rnode-main")
        .arg("--vport")
        .arg("2")
        .arg("--confirm-persistent")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["vport"].as_u64(), Some(2));
    assert_eq!(value["confirmation"].as_str(), Some("persistent"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_cli_covers_extended_python_management_surface() {
    let cases = vec![
        (
            vec!["read-rom", "--interface", "rnode-main"],
            "rom_read",
            json!({}),
            json!({ "queued": true, "command": "rom_read" }),
        ),
        (
            vec!["set-display-blanking", "--interface", "rnode-main", "--timeout", "12"],
            "display_blanking",
            json!({ "timeout": 12 }),
            json!({ "queued": true, "command": "display_blanking", "blanking_timeout": 12 }),
        ),
        (
            vec!["set-display-rotation", "--interface", "rnode-main", "--rotation", "2"],
            "display_rotation",
            json!({ "rotation": 2 }),
            json!({ "queued": true, "command": "display_rotation", "rotation": 2 }),
        ),
        (
            vec!["recondition-display", "--interface", "rnode-main"],
            "display_recondition",
            json!({}),
            json!({ "queued": true, "command": "display_recondition" }),
        ),
        (
            vec!["set-display-address", "--interface", "rnode-main", "--address", "60"],
            "display_address",
            json!({ "address": 60 }),
            json!({ "queued": true, "command": "display_address", "address": 60 }),
        ),
        (
            vec!["set-neopixel-intensity", "--interface", "rnode-main", "--intensity", "4"],
            "neopixel_intensity",
            json!({ "intensity": 4 }),
            json!({ "queued": true, "command": "neopixel_intensity", "intensity": 4 }),
        ),
        (
            vec!["enable-interference-avoidance", "--interface", "rnode-main"],
            "enable_interference_avoidance",
            json!({}),
            json!({ "queued": true, "command": "disable_interference_avoidance", "disabled": false }),
        ),
        (
            vec!["enable-bluetooth", "--interface", "rnode-main", "--confirm-persistent"],
            "bluetooth_enable",
            json!({ "confirm_persistent": true }),
            json!({ "queued": true, "command": "bluetooth_enable", "confirmation": "persistent" }),
        ),
        (
            vec!["disable-bluetooth", "--interface", "rnode-main", "--confirm-persistent"],
            "bluetooth_disable",
            json!({ "confirm_persistent": true }),
            json!({ "queued": true, "command": "bluetooth_disable", "confirmation": "persistent" }),
        ),
        (
            vec!["pair-bluetooth", "--interface", "rnode-main", "--confirm-persistent"],
            "bluetooth_pair",
            json!({ "confirm_persistent": true }),
            json!({ "queued": true, "command": "bluetooth_pair", "confirmation": "persistent" }),
        ),
        (
            vec![
                "delete-config",
                "--interface",
                "rnode-main",
                "--confirm-destructive",
                "--confirm-command",
                "config_delete",
            ],
            "config_delete",
            json!({ "confirm_destructive": true, "confirm_command": "config_delete" }),
            json!({ "queued": true, "command": "config_delete", "confirmation": "destructive" }),
        ),
        (
            vec![
                "wipe-rom",
                "--interface",
                "rnode-main",
                "--confirm-destructive",
                "--confirm-command",
                "rom_wipe",
            ],
            "rom_wipe",
            json!({ "confirm_destructive": true, "confirm_command": "rom_wipe" }),
            json!({ "queued": true, "command": "rom_wipe", "confirmation": "destructive" }),
        ),
        (
            vec![
                "hard-reset",
                "--interface",
                "rnode-main",
                "--confirm-destructive",
                "--confirm-command",
                "hard_reset",
            ],
            "hard_reset",
            json!({ "confirm_destructive": true, "confirm_command": "hard_reset" }),
            json!({ "queued": true, "command": "hard_reset", "confirmation": "destructive" }),
        ),
        (
            vec!["firmware-update", "--interface", "rnode-main", "--confirm-persistent"],
            "firmware_update_indicator",
            json!({ "confirm_persistent": true }),
            json!({ "queued": true, "command": "firmware_update_indicator", "confirmation": "persistent" }),
        ),
        (
            vec![
                "set-firmware-hash",
                "--interface",
                "rnode-main",
                "--hash-hex",
                "c0ffee",
                "--confirm-persistent",
            ],
            "firmware_hash",
            json!({ "hash_hex": "c0ffee", "confirm_persistent": true }),
            json!({
                "queued": true,
                "command": "firmware_hash",
                "hash_hex": "c0ffee",
                "confirmation": "persistent"
            }),
        ),
        (
            vec![
                "set-wifi-mode",
                "--interface",
                "rnode-main",
                "--mode",
                "2",
                "--confirm-persistent",
            ],
            "wifi_mode",
            json!({ "mode": 2, "confirm_persistent": true }),
            json!({ "queued": true, "command": "wifi_mode", "mode": 2, "confirmation": "persistent" }),
        ),
        (
            vec![
                "set-wifi-channel",
                "--interface",
                "rnode-main",
                "--channel",
                "11",
                "--confirm-persistent",
            ],
            "wifi_channel",
            json!({ "channel": 11, "confirm_persistent": true }),
            json!({ "queued": true, "command": "wifi_channel", "channel": 11, "confirmation": "persistent" }),
        ),
        (
            vec![
                "set-wifi-ip",
                "--interface",
                "rnode-main",
                "--ip",
                "192.168.4.1",
                "--confirm-persistent",
            ],
            "wifi_ip",
            json!({ "ip": "192.168.4.1", "confirm_persistent": true }),
            json!({ "queued": true, "command": "wifi_ip", "ip": "192.168.4.1", "confirmation": "persistent" }),
        ),
        (
            vec!["clear-wifi-ip", "--interface", "rnode-main", "--confirm-persistent"],
            "clear_wifi_ip",
            json!({ "confirm_persistent": true }),
            json!({ "queued": true, "command": "wifi_ip", "ip": null, "confirmation": "persistent" }),
        ),
        (
            vec![
                "set-wifi-netmask",
                "--interface",
                "rnode-main",
                "--netmask",
                "255.255.255.0",
                "--confirm-persistent",
            ],
            "wifi_netmask",
            json!({ "netmask": "255.255.255.0", "confirm_persistent": true }),
            json!({
                "queued": true,
                "command": "wifi_netmask",
                "netmask": "255.255.255.0",
                "confirmation": "persistent"
            }),
        ),
        (
            vec!["clear-wifi-netmask", "--interface", "rnode-main", "--confirm-persistent"],
            "clear_wifi_netmask",
            json!({ "confirm_persistent": true }),
            json!({ "queued": true, "command": "wifi_netmask", "netmask": null, "confirmation": "persistent" }),
        ),
        (
            vec![
                "set-wifi-ssid",
                "--interface",
                "rnode-main",
                "--ssid",
                "field-net",
                "--confirm-persistent",
            ],
            "wifi_ssid",
            json!({ "ssid": "field-net", "confirm_persistent": true }),
            json!({ "queued": true, "command": "wifi_ssid", "ssid": "field-net", "confirmation": "persistent" }),
        ),
        (
            vec!["clear-wifi-ssid", "--interface", "rnode-main", "--confirm-persistent"],
            "clear_wifi_ssid",
            json!({ "confirm_persistent": true }),
            json!({ "queued": true, "command": "wifi_ssid", "ssid": null, "confirmation": "persistent" }),
        ),
        (
            vec!["clear-wifi-psk", "--interface", "rnode-main", "--confirm-persistent"],
            "clear_wifi_psk",
            json!({ "confirm_persistent": true }),
            json!({ "queued": true, "command": "wifi_psk", "psk_set": false, "confirmation": "persistent" }),
        ),
    ];

    for (args, expected_command, expected_params, result) in cases {
        let rpc = spawn_mock_rpc(move |request| {
            assert_eq!(request.method, "rnode_management");
            let params = request.params.expect("params");
            assert_eq!(params["iface"].as_str(), Some("rnode-main"));
            assert_eq!(params["command"].as_str(), Some(expected_command));
            assert_json_subset(&params, &expected_params);
            RpcResponse { id: request.id, result: Some(result), error: None }
        });

        let output = Command::new(rnodeconf_bin())
            .arg("--rpc")
            .arg(rpc.addr)
            .args(args)
            .output()
            .expect("run rnodeconf-rs");

        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
        let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
        let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
        assert!(value["queued"].as_bool() == Some(true), "stdout: {stdout}");
        rpc.thread.join().expect("mock rpc server");
    }
}

#[test]
fn rnodeconf_rejects_missing_management_confirmation_before_rpc() {
    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg("127.0.0.1:9")
        .arg("save-config")
        .arg("--interface")
        .arg("rnode-main")
        .output()
        .expect("run rnodeconf-rs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("requires --confirm-persistent"), "{stderr}");
    assert!(!stderr.contains("Connection refused"), "{stderr}");
}

fn assert_json_subset(actual: &JsonValue, expected: &JsonValue) {
    let expected = expected.as_object().expect("expected params object");
    for (key, expected_value) in expected {
        assert_eq!(actual.get(key), Some(expected_value), "mismatch for key {key}");
    }
}

struct MockRpc {
    addr: String,
    thread: thread::JoinHandle<()>,
}

fn spawn_mock_rpc<F>(handler: F) -> MockRpc
where
    F: FnOnce(rns_rpc::RpcRequest) -> RpcResponse + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc");
    let addr = listener.local_addr().expect("mock rpc addr").to_string();
    let thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        let response = handler(rpc_request);
        let body = codec::encode_frame(&response).expect("encode response");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write response headers");
        stream.write_all(&body).expect("write response body");
        stream.shutdown(Shutdown::Write).expect("shutdown response");
    });
    MockRpc { addr, thread }
}

fn rnodeconf_bin() -> String {
    env!("CARGO_BIN_EXE_rnodeconf-rs").to_string()
}

fn http_body(request: &[u8]) -> &[u8] {
    let marker = b"\r\n\r\n";
    let start = request
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|index| index + marker.len())
        .expect("request headers");
    &request[start..]
}
