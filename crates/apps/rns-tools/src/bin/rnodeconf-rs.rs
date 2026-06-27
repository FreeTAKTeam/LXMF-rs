use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};

use clap::{Args, Parser, Subcommand};
use rns_rpc::e2e_harness::{build_http_post, build_rpc_frame, parse_http_response_body};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(name = "rnodeconf-rs")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:4243")]
    rpc: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    QueryRadioState {
        #[command(flatten)]
        target: Target,
    },
    ReadConfig {
        #[command(flatten)]
        target: Target,
    },
    ReadRom {
        #[command(flatten)]
        target: Target,
    },
    Blink {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        pattern: u8,
    },
    SetDisplayIntensity {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        intensity: u8,
    },
    SetDisplayBlanking {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        timeout: u8,
    },
    SetDisplayRotation {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        rotation: u8,
    },
    ReconditionDisplay {
        #[command(flatten)]
        target: Target,
    },
    SetDisplayAddress {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        address: u8,
    },
    SetNeopixelIntensity {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        intensity: u8,
    },
    DisableInterferenceAvoidance {
        #[command(flatten)]
        target: Target,
    },
    EnableInterferenceAvoidance {
        #[command(flatten)]
        target: Target,
    },
    EnableBluetooth {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_persistent: bool,
    },
    DisableBluetooth {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_persistent: bool,
    },
    PairBluetooth {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_persistent: bool,
    },
    SaveConfig {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_persistent: bool,
    },
    DeleteConfig {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_destructive: bool,
        #[arg(long)]
        confirm_command: String,
    },
    WriteRom {
        #[command(flatten)]
        target: Target,
        #[arg(long, alias = "addr")]
        address: u8,
        #[arg(long, alias = "value")]
        byte: u8,
        #[arg(long)]
        confirm_destructive: bool,
        #[arg(long)]
        confirm_command: String,
    },
    WipeRom {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_destructive: bool,
        #[arg(long)]
        confirm_command: String,
    },
    HardReset {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_destructive: bool,
        #[arg(long)]
        confirm_command: String,
    },
    FirmwareUpdate {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_persistent: bool,
    },
    SetFirmwareHash {
        #[command(flatten)]
        target: Target,
        #[arg(long = "hash-hex")]
        hash_hex: String,
        #[arg(long)]
        confirm_persistent: bool,
    },
    SetWifiMode {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        mode: u8,
        #[arg(long)]
        confirm_persistent: bool,
    },
    SetWifiChannel {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        channel: u8,
        #[arg(long)]
        confirm_persistent: bool,
    },
    SetWifiIp {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        ip: String,
        #[arg(long)]
        confirm_persistent: bool,
    },
    ClearWifiIp {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_persistent: bool,
    },
    SetWifiNetmask {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        netmask: String,
        #[arg(long)]
        confirm_persistent: bool,
    },
    ClearWifiNetmask {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_persistent: bool,
    },
    SetWifiSsid {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        ssid: String,
        #[arg(long)]
        confirm_persistent: bool,
    },
    ClearWifiSsid {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_persistent: bool,
    },
    SetWifiPsk {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        psk: String,
        #[arg(long)]
        confirm_persistent: bool,
    },
    ClearWifiPsk {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        confirm_persistent: bool,
    },
}

#[derive(Debug, Args)]
struct Target {
    #[arg(long = "interface")]
    iface: String,
    #[arg(long)]
    vport: Option<u8>,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(&cli, &mut io::stdout()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rnodeconf-rs: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli, output: &mut dyn Write) -> io::Result<()> {
    let params = match &cli.command {
        Command::QueryRadioState { target } => rpc_params(target, "radio_state_query", json!({})),
        Command::ReadConfig { target } => rpc_params(target, "config_read", json!({})),
        Command::ReadRom { target } => rpc_params(target, "rom_read", json!({})),
        Command::Blink { target, pattern } => {
            rpc_params(target, "blink", json!({ "pattern": pattern }))
        }
        Command::SetDisplayIntensity { target, intensity } => {
            rpc_params(target, "display_intensity", json!({ "intensity": intensity }))
        }
        Command::SetDisplayBlanking { target, timeout } => {
            rpc_params(target, "display_blanking", json!({ "timeout": timeout }))
        }
        Command::SetDisplayRotation { target, rotation } => {
            rpc_params(target, "display_rotation", json!({ "rotation": rotation }))
        }
        Command::ReconditionDisplay { target } => {
            rpc_params(target, "display_recondition", json!({}))
        }
        Command::SetDisplayAddress { target, address } => {
            rpc_params(target, "display_address", json!({ "address": address }))
        }
        Command::SetNeopixelIntensity { target, intensity } => {
            rpc_params(target, "neopixel_intensity", json!({ "intensity": intensity }))
        }
        Command::DisableInterferenceAvoidance { target } => {
            rpc_params(target, "disable_interference_avoidance", json!({ "disabled": true }))
        }
        Command::EnableInterferenceAvoidance { target } => {
            rpc_params(target, "enable_interference_avoidance", json!({}))
        }
        Command::EnableBluetooth { target, confirm_persistent } => rpc_params(
            target,
            "bluetooth_enable",
            json!({ "confirm_persistent": confirm_persistent }),
        ),
        Command::DisableBluetooth { target, confirm_persistent } => rpc_params(
            target,
            "bluetooth_disable",
            json!({ "confirm_persistent": confirm_persistent }),
        ),
        Command::PairBluetooth { target, confirm_persistent } => rpc_params(
            target,
            "bluetooth_pair",
            json!({ "confirm_persistent": confirm_persistent }),
        ),
        Command::SaveConfig { target, confirm_persistent } => {
            rpc_params(target, "config_save", json!({ "confirm_persistent": confirm_persistent }))
        }
        Command::DeleteConfig { target, confirm_destructive, confirm_command } => rpc_params(
            target,
            "config_delete",
            json!({
                "confirm_destructive": confirm_destructive,
                "confirm_command": confirm_command,
            }),
        ),
        Command::WriteRom { target, address, byte, confirm_destructive, confirm_command } => {
            rpc_params(
                target,
                "rom_write",
                json!({
                    "address": address,
                    "byte": byte,
                    "confirm_destructive": confirm_destructive,
                    "confirm_command": confirm_command,
                }),
            )
        }
        Command::WipeRom { target, confirm_destructive, confirm_command } => rpc_params(
            target,
            "rom_wipe",
            json!({
                "confirm_destructive": confirm_destructive,
                "confirm_command": confirm_command,
            }),
        ),
        Command::HardReset { target, confirm_destructive, confirm_command } => rpc_params(
            target,
            "hard_reset",
            json!({
                "confirm_destructive": confirm_destructive,
                "confirm_command": confirm_command,
            }),
        ),
        Command::FirmwareUpdate { target, confirm_persistent } => rpc_params(
            target,
            "firmware_update_indicator",
            json!({ "confirm_persistent": confirm_persistent }),
        ),
        Command::SetFirmwareHash { target, hash_hex, confirm_persistent } => rpc_params(
            target,
            "firmware_hash",
            json!({ "hash_hex": hash_hex, "confirm_persistent": confirm_persistent }),
        ),
        Command::SetWifiMode { target, mode, confirm_persistent } => rpc_params(
            target,
            "wifi_mode",
            json!({ "mode": mode, "confirm_persistent": confirm_persistent }),
        ),
        Command::SetWifiChannel { target, channel, confirm_persistent } => rpc_params(
            target,
            "wifi_channel",
            json!({ "channel": channel, "confirm_persistent": confirm_persistent }),
        ),
        Command::SetWifiIp { target, ip, confirm_persistent } => rpc_params(
            target,
            "wifi_ip",
            json!({ "ip": ip, "confirm_persistent": confirm_persistent }),
        ),
        Command::ClearWifiIp { target, confirm_persistent } => {
            rpc_params(target, "clear_wifi_ip", json!({ "confirm_persistent": confirm_persistent }))
        }
        Command::SetWifiNetmask { target, netmask, confirm_persistent } => rpc_params(
            target,
            "wifi_netmask",
            json!({ "netmask": netmask, "confirm_persistent": confirm_persistent }),
        ),
        Command::ClearWifiNetmask { target, confirm_persistent } => rpc_params(
            target,
            "clear_wifi_netmask",
            json!({ "confirm_persistent": confirm_persistent }),
        ),
        Command::SetWifiSsid { target, ssid, confirm_persistent } => rpc_params(
            target,
            "wifi_ssid",
            json!({ "ssid": ssid, "confirm_persistent": confirm_persistent }),
        ),
        Command::ClearWifiSsid { target, confirm_persistent } => rpc_params(
            target,
            "clear_wifi_ssid",
            json!({ "confirm_persistent": confirm_persistent }),
        ),
        Command::SetWifiPsk { target, psk, confirm_persistent } => rpc_params(
            target,
            "wifi_psk",
            json!({ "psk": psk, "confirm_persistent": confirm_persistent }),
        ),
        Command::ClearWifiPsk { target, confirm_persistent } => rpc_params(
            target,
            "clear_wifi_psk",
            json!({ "confirm_persistent": confirm_persistent }),
        ),
    };
    validate_cli_guards(&params)?;
    let response = rpc_call(&cli.rpc, 1, "rnode_management", Some(params))?;
    let result = ensure_rpc_ok(response, "rnode_management")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing RNode result"))?;
    writeln!(output, "{}", serde_json::to_string_pretty(&result)?)?;
    Ok(())
}

fn rpc_params(target: &Target, command: &str, mut extra: serde_json::Value) -> serde_json::Value {
    let Some(map) = extra.as_object_mut() else {
        return json!({
            "iface": target.iface,
            "command": command,
            "vport": target.vport,
        });
    };
    map.insert("iface".to_string(), json!(target.iface));
    map.insert("command".to_string(), json!(command));
    if let Some(vport) = target.vport {
        map.insert("vport".to_string(), json!(vport));
    }
    extra
}

fn validate_cli_guards(params: &serde_json::Value) -> io::Result<()> {
    let command = params.get("command").and_then(serde_json::Value::as_str).unwrap_or_default();
    if is_persistent_command(command)
        && params.get("confirm_persistent").and_then(serde_json::Value::as_bool) != Some(true)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{command} requires --confirm-persistent"),
        ));
    }
    if is_destructive_command(command) {
        if params.get("confirm_destructive").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{command} requires --confirm-destructive"),
            ));
        }
        if params.get("confirm_command").and_then(serde_json::Value::as_str) != Some(command) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{command} requires --confirm-command {command}"),
            ));
        }
    }
    Ok(())
}

fn is_persistent_command(command: &str) -> bool {
    matches!(
        command,
        "bluetooth_enable"
            | "bluetooth_disable"
            | "bluetooth_pair"
            | "config_save"
            | "firmware_update_indicator"
            | "firmware_hash"
            | "wifi_mode"
            | "wifi_channel"
            | "wifi_ip"
            | "clear_wifi_ip"
            | "wifi_netmask"
            | "clear_wifi_netmask"
            | "wifi_ssid"
            | "clear_wifi_ssid"
            | "wifi_psk"
            | "clear_wifi_psk"
    )
}

fn is_destructive_command(command: &str) -> bool {
    matches!(command, "config_delete" | "rom_write" | "rom_wipe" | "hard_reset")
}

fn rpc_call(
    rpc: &str,
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<rns_rpc::RpcResponse> {
    let frame = build_rpc_frame(id, method, params)?;
    let request = build_http_post("/rpc", rpc, &frame);
    let mut stream = TcpStream::connect(rpc)?;
    stream.write_all(&request)?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let body = parse_http_response_body(&response)?;
    rns_rpc::rpc::codec::decode_frame(&body)
}

fn ensure_rpc_ok(
    response: rns_rpc::RpcResponse,
    context: &str,
) -> io::Result<Option<serde_json::Value>> {
    if let Some(error) = response.error {
        return Err(io::Error::other(format!(
            "{} failed: {} ({})",
            context, error.message, error.code
        )));
    }
    Ok(response.result)
}
