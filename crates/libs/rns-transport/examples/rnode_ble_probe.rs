use rns_transport::iface::lora::{
    LoraConfig, RNodeProbeStatus, RNodeRadioStatus, CMD_DETECT, CMD_FW_VERSION, CMD_MCU,
    CMD_PLATFORM, DETECT_REQ, PLATFORM_AVR, PLATFORM_ESP32, PLATFORM_NRF52, RADIO_STATE_OFF,
    RADIO_STATE_ON, REQUIRED_FW_VERSION_MAJOR, REQUIRED_FW_VERSION_MINOR,
};
use rns_transport::iface::rnode_ble::{
    NativeRnodeBleBackend, NativeRnodeBleSettings, RnodeBleKissConfig, RnodeBleKissError,
    RnodeBleKissRuntime,
};
use rns_transport::kiss::encode_command_frame;
use std::time::Duration;
use tokio::time::Instant;

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("rnode_ble_probe: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args = ProbeArgs::parse(std::env::args().skip(1))?;
    if args.help {
        print_usage();
        return Ok(());
    }

    let lora_config = build_lora_config(&args)?;

    let initial_frames = match lora_config.as_ref() {
        Some(lc) => lc.command_frames(),
        None => vec![
            encode_command_frame(CMD_DETECT, &[DETECT_REQ]),
            encode_command_frame(CMD_FW_VERSION, &[0x00]),
            encode_command_frame(CMD_PLATFORM, &[0x00]),
            encode_command_frame(CMD_MCU, &[0x00]),
        ],
    };
    let shutdown_frames = lora_config.as_ref().map(|lc| lc.shutdown_frames()).unwrap_or_default();

    let mut settings = NativeRnodeBleSettings::for_peripheral(args.peripheral_id.clone());
    settings.scan_timeout = Duration::from_millis(args.scan_timeout_ms);
    settings.connect_timeout = Duration::from_millis(args.connect_timeout_ms);
    settings.notification_timeout = Duration::from_millis(500);
    if let Some(adapter) = args.adapter.as_deref() {
        settings = settings.with_adapter(adapter.to_string());
    }

    let config =
        RnodeBleKissConfig { initial_frames, shutdown_frames, ..RnodeBleKissConfig::default() };

    let backend = NativeRnodeBleBackend::new(settings);
    let mut runtime = RnodeBleKissRuntime::new(backend, config);

    println!("scanning for '{}'...", args.peripheral_id);
    runtime.startup().await.map_err(|err| format!("startup failed: {err:?}"))?;
    println!("connected");

    let mut probe = RNodeProbeStatus::default();
    let mut radio = RNodeRadioStatus::default();
    poll_notifications(
        &mut runtime,
        Duration::from_millis(args.startup_timeout_ms),
        &mut probe,
        &mut radio,
        false,
    )
    .await;

    println!("\n--- probe status ---");
    print_probe_status(&probe);
    if lora_config.is_some() {
        println!("\n--- radio status ---");
        print_radio_status(&radio);
    }
    if let Err(err) = validate_probe_result(&probe, &radio, lora_config) {
        shutdown_and_cleanup(runtime).await;
        return Err(err);
    }

    if let Some(hex) = args.send_hex.as_deref() {
        let bytes = parse_hex(hex)?;
        runtime.send_packet(&bytes).await.map_err(|err| format!("send failed: {err:?}"))?;
        println!("\nsent {} byte(s): {}", bytes.len(), hex::encode(&bytes));
    }

    if args.listen_secs > 0 {
        println!("\nlistening for {} second(s)...", args.listen_secs);
        poll_notifications(
            &mut runtime,
            Duration::from_secs(args.listen_secs),
            &mut probe,
            &mut radio,
            true,
        )
        .await;
    }

    shutdown_and_cleanup(runtime).await;
    Ok(())
}

async fn shutdown_and_cleanup(mut runtime: RnodeBleKissRuntime<NativeRnodeBleBackend>) {
    let _ = runtime.shutdown().await;
    let mut backend = runtime.into_backend();
    if let Err(err) = backend.cleanup().await {
        eprintln!("cleanup warning: {err}");
    }
}

fn validate_probe_result(
    probe: &RNodeProbeStatus,
    radio: &RNodeRadioStatus,
    lora_config: Option<LoraConfig>,
) -> Result<(), String> {
    probe.validate_startup_probe()?;
    if let Some(config) = lora_config {
        radio.validate_config(config, RADIO_STATE_ON)?;
    }
    Ok(())
}

async fn poll_notifications(
    runtime: &mut RnodeBleKissRuntime<NativeRnodeBleBackend>,
    duration: Duration,
    probe: &mut RNodeProbeStatus,
    radio: &mut RNodeRadioStatus,
    print_packets: bool,
) {
    let end = Instant::now() + duration;
    loop {
        if Instant::now() >= end {
            break;
        }
        match runtime.poll_notification_events().await {
            Ok(notification) => {
                for (cmd, payload) in &notification.commands {
                    let _ = probe.accept_command(*cmd, payload);
                    let _ = radio.accept_command(*cmd, payload);
                }
                if print_packets {
                    for packet in &notification.packets {
                        println!("received {} byte(s): {}", packet.len(), hex::encode(packet));
                    }
                }
            }
            Err(RnodeBleKissError::Backend { message, .. }) => {
                if !message.contains("timeout") {
                    eprintln!("poll warning: {message}");
                }
            }
            Err(err) => eprintln!("poll warning: {err:?}"),
        }
    }
}

fn build_lora_config(args: &ProbeArgs) -> Result<Option<LoraConfig>, String> {
    if args.region.is_none()
        && args.freq_hz.is_none()
        && args.bw_hz.is_none()
        && args.txpower.is_none()
        && args.sf.is_none()
        && args.cr.is_none()
    {
        return Ok(None);
    }
    let base = match args.region.as_deref() {
        Some(r) => LoraConfig::for_region(r)
            .ok()
            .flatten()
            .ok_or_else(|| format!("unknown region '{r}'; try EU868, US915, AU915, AS923"))?,
        None => LoraConfig::us915_default(),
    };
    let config = LoraConfig {
        frequency_hz: args.freq_hz.unwrap_or(base.frequency_hz),
        bandwidth_hz: args.bw_hz.unwrap_or(base.bandwidth_hz),
        tx_power_dbm: args.txpower.unwrap_or(base.tx_power_dbm),
        spreading_factor: args.sf.unwrap_or(base.spreading_factor),
        coding_rate: args.cr.unwrap_or(base.coding_rate),
        ..base
    };
    config.validate().map_err(|e| format!("radio config invalid: {e}"))?;
    Ok(Some(config))
}

fn print_probe_status(probe: &RNodeProbeStatus) {
    println!("detected:         {}", if probe.detected { "yes" } else { "no" });
    match probe.firmware_version {
        Some((maj, min)) => {
            let ok = maj > REQUIRED_FW_VERSION_MAJOR
                || (maj == REQUIRED_FW_VERSION_MAJOR && min >= REQUIRED_FW_VERSION_MINOR);
            let note = if ok { "" } else { " [WARN: below required]" };
            println!("firmware:         {maj}.{min}{note}");
        }
        None => println!("firmware:         not reported"),
    }
    let platform = match probe.platform {
        Some(PLATFORM_ESP32) => "ESP32".to_string(),
        Some(PLATFORM_NRF52) => "NRF52".to_string(),
        Some(PLATFORM_AVR) => "AVR".to_string(),
        Some(p) => format!("unknown (0x{p:02x})"),
        None => "not reported".to_string(),
    };
    println!("platform:         {platform}");
    match probe.mcu {
        Some(mcu) => println!("mcu:              0x{mcu:02x}"),
        None => println!("mcu:              not reported"),
    }
}

fn print_radio_status(radio: &RNodeRadioStatus) {
    if let Some(hz) = radio.frequency_hz {
        println!("frequency:        {:.3} MHz", hz as f64 / 1_000_000.0);
    }
    if let Some(hz) = radio.bandwidth_hz {
        println!("bandwidth:        {:.1} kHz", hz as f64 / 1_000.0);
    }
    if let Some(sf) = radio.spreading_factor {
        println!("spreading factor: {sf}");
    }
    if let Some(cr) = radio.coding_rate {
        println!("coding rate:      {cr}");
    }
    if let Some(dbm) = radio.tx_power_dbm {
        println!("tx power:         {dbm} dBm");
    }
    match radio.radio_state {
        Some(RADIO_STATE_ON) => println!("radio state:      ON"),
        Some(RADIO_STATE_OFF) => println!("radio state:      OFF"),
        Some(s) => println!("radio state:      0x{s:02x}"),
        None => {}
    }
    if let Some(rssi) = radio.rssi_dbm {
        println!("last RSSI:        {rssi} dBm");
    }
    if let Some(snr) = radio.snr_db {
        println!("last SNR:         {snr:.2} dB");
    }
    if let Some(temp) = radio.temperature_c {
        println!("temperature:      {temp} °C");
    }
    if let Some(bat) = radio.battery_percent {
        println!("battery:          {} ({bat}%)", radio.battery_state_string());
    }
}

struct ProbeArgs {
    peripheral_id: String,
    adapter: Option<String>,
    scan_timeout_ms: u64,
    connect_timeout_ms: u64,
    startup_timeout_ms: u64,
    region: Option<String>,
    freq_hz: Option<u64>,
    bw_hz: Option<u32>,
    txpower: Option<i8>,
    sf: Option<u8>,
    cr: Option<u8>,
    listen_secs: u64,
    send_hex: Option<String>,
    help: bool,
}

impl ProbeArgs {
    fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut parsed = Self {
            peripheral_id: String::new(),
            adapter: None,
            scan_timeout_ms: 10_000,
            connect_timeout_ms: 5_000,
            startup_timeout_ms: 3_000,
            region: None,
            freq_hz: None,
            bw_hz: None,
            txpower: None,
            sf: None,
            cr: None,
            listen_secs: 0,
            send_hex: None,
            help: false,
        };
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "-h" | "--help" => parsed.help = true,
                "--peripheral-id" => parsed.peripheral_id = next_val(&mut it, &arg)?,
                "--adapter" => parsed.adapter = Some(next_val(&mut it, &arg)?),
                "--scan-timeout-ms" => parsed.scan_timeout_ms = parse_next(&mut it, &arg)?,
                "--connect-timeout-ms" => parsed.connect_timeout_ms = parse_next(&mut it, &arg)?,
                "--startup-timeout-ms" => parsed.startup_timeout_ms = parse_next(&mut it, &arg)?,
                "--region" => parsed.region = Some(next_val(&mut it, &arg)?),
                "--freq-hz" => parsed.freq_hz = Some(parse_next(&mut it, &arg)?),
                "--bw-hz" => parsed.bw_hz = Some(parse_next(&mut it, &arg)?),
                "--txpower" => parsed.txpower = Some(parse_next(&mut it, &arg)?),
                "--sf" => parsed.sf = Some(parse_next(&mut it, &arg)?),
                "--cr" => parsed.cr = Some(parse_next(&mut it, &arg)?),
                "--listen-secs" => parsed.listen_secs = parse_next(&mut it, &arg)?,
                "--send-hex" => parsed.send_hex = Some(next_val(&mut it, &arg)?),
                _ => return Err(format!("unknown argument '{arg}'")),
            }
        }
        if parsed.help {
            return Ok(parsed);
        }
        if parsed.peripheral_id.trim().is_empty() {
            return Err("--peripheral-id is required".to_string());
        }
        if parsed.scan_timeout_ms == 0 {
            return Err("--scan-timeout-ms must be > 0".to_string());
        }
        if parsed.connect_timeout_ms == 0 {
            return Err("--connect-timeout-ms must be > 0".to_string());
        }
        Ok(parsed)
    }
}

fn next_val<I: Iterator<Item = String>>(it: &mut I, flag: &str) -> Result<String, String> {
    it.next().ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_next<I, T>(it: &mut I, flag: &str) -> Result<T, String>
where
    I: Iterator<Item = String>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    next_val(it, flag)?.parse::<T>().map_err(|e| format!("{flag} has invalid value: {e}"))
}

fn parse_hex(value: &str) -> Result<Vec<u8>, String> {
    let normalized = value.trim().replace([' ', ':', '-'], "");
    if normalized.len() % 2 != 0 {
        return Err("hex must contain an even number of digits".to_string());
    }
    hex::decode(&normalized).map_err(|e| format!("invalid hex: {e}"))
}

fn print_usage() {
    println!(
        "Usage: cargo run -p reticulum-rs-transport --features rnode-ble \
--example rnode_ble_probe -- --peripheral-id <name|addr> [options]\n\n\
Options:\n  \
--adapter <name>              Match a specific host BLE adapter\n  \
--scan-timeout-ms <ms>        BLE scan timeout (default 10000)\n  \
--connect-timeout-ms <ms>     BLE connect timeout (default 5000)\n  \
--startup-timeout-ms <ms>     Time to collect probe responses (default 3000)\n  \
--region <name>               LoRa region: EU868 US915 AU915 AS923 IN865 KR920 RU864\n  \
--freq-hz <hz>                Override frequency in Hz (e.g. 868000000)\n  \
--bw-hz <hz>                  Bandwidth in Hz (e.g. 125000)\n  \
--txpower <dbm>               TX power in dBm (0-37)\n  \
--sf <5-12>                   Spreading factor\n  \
--cr <5-8>                    Coding rate\n  \
--listen-secs <n>             Listen for incoming LoRa packets for N seconds\n  \
--send-hex <hex>              Send a LoRa packet with this hex payload\n\n\
If no radio options are given, only connectivity and firmware are probed."
    );
}
