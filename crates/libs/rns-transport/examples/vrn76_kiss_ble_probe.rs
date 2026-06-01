use rns_transport::iface::vrn76_kiss_ble::{
    NativeVrn76BleBackend, NativeVrn76BleSettings, Vrn76KissBleConfig, Vrn76KissBleRuntime,
};
use std::time::Duration;

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("vrn76_kiss_ble_probe: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args = ProbeArgs::parse(std::env::args().skip(1))?;
    if args.help {
        print_usage();
        return Ok(());
    }

    let mut settings = NativeVrn76BleSettings::for_peripheral(args.peripheral_id.clone());
    settings.scan_timeout = Duration::from_millis(args.scan_timeout_ms);
    settings.connect_timeout = Duration::from_millis(args.command_timeout_ms);
    settings.notification_timeout = Duration::from_millis(args.command_timeout_ms);
    if let Some(adapter) = args.adapter.as_deref() {
        settings = settings.with_adapter(adapter.to_string());
    }

    let config = Vrn76KissBleConfig {
        mtu: args.mtu,
        scan_timeout: Duration::from_millis(args.scan_timeout_ms),
        command_timeout: Duration::from_millis(args.command_timeout_ms),
        kiss: rns_transport::iface::kiss::KissConfig {
            preamble_ms: args.preamble_ms,
            tx_tail_ms: args.tx_tail_ms,
            persistence: args.persistence,
            slot_time_ms: args.slot_time_ms,
            flow_control: args.flow_control,
            id_beacon: None,
        },
        ..Vrn76KissBleConfig::default()
    };

    println!(
        "scanning for VR-N76-compatible peripheral '{}'{}",
        args.peripheral_id,
        args.adapter
            .as_deref()
            .map(|adapter| format!(" on adapter '{adapter}'"))
            .unwrap_or_default()
    );
    let backend = NativeVrn76BleBackend::new(settings);
    let mut runtime = Vrn76KissBleRuntime::new(backend, config);
    runtime
        .connect_and_configure()
        .await
        .map_err(|err| format!("connect/configure failed: {err:?}"))?;
    let status = runtime.status();
    println!(
        "connected={} subscribed={} interface_ready={} startup_write_failures={} pending_payloads={} pending_writes={} pending_packets={}",
        status.connected,
        status.subscribed,
        status.interface_ready,
        status.startup_write_failures,
        status.pending_payloads,
        status.pending_writes,
        status.pending_packets
    );

    if let Some(payload) = args.test_payload.as_deref() {
        runtime
            .send_packet(payload)
            .await
            .map_err(|err| format!("test KISS frame write failed: {err:?}"))?;
        let status = runtime.status();
        println!("sent explicit test KISS data frame with {} payload byte(s)", payload.len());
        println!(
            "connected={} subscribed={} interface_ready={} startup_write_failures={} pending_payloads={} pending_writes={} pending_packets={}",
            status.connected,
            status.subscribed,
            status.interface_ready,
            status.startup_write_failures,
            status.pending_payloads,
            status.pending_writes,
            status.pending_packets
        );
    } else {
        println!("test KISS frame not sent; pass --send-test-kiss-frame to transmit one");
    }

    let mut backend = runtime.into_backend();
    if let Err(err) = backend.cleanup().await {
        eprintln!("cleanup warning: {err}");
    }
    Ok(())
}

#[derive(Debug)]
struct ProbeArgs {
    peripheral_id: String,
    adapter: Option<String>,
    scan_timeout_ms: u64,
    command_timeout_ms: u64,
    mtu: usize,
    preamble_ms: u16,
    tx_tail_ms: u16,
    persistence: u8,
    slot_time_ms: u16,
    flow_control: bool,
    test_payload: Option<Vec<u8>>,
    help: bool,
}

impl ProbeArgs {
    fn parse<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut parsed = Self {
            peripheral_id: String::new(),
            adapter: None,
            scan_timeout_ms: 10_000,
            command_timeout_ms: 3_000,
            mtu: 564,
            preamble_ms: 350,
            tx_tail_ms: 20,
            persistence: 64,
            slot_time_ms: 20,
            flow_control: false,
            test_payload: None,
            help: false,
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => parsed.help = true,
                "--peripheral-id" => parsed.peripheral_id = next_value(&mut args, &arg)?,
                "--adapter" => parsed.adapter = Some(next_value(&mut args, &arg)?),
                "--scan-timeout-ms" => {
                    parsed.scan_timeout_ms = parse_next(&mut args, &arg)?;
                }
                "--command-timeout-ms" => {
                    parsed.command_timeout_ms = parse_next(&mut args, &arg)?;
                }
                "--mtu" => parsed.mtu = parse_next(&mut args, &arg)?,
                "--preamble-ms" => parsed.preamble_ms = parse_next(&mut args, &arg)?,
                "--tx-tail-ms" => parsed.tx_tail_ms = parse_next(&mut args, &arg)?,
                "--persistence" => parsed.persistence = parse_next(&mut args, &arg)?,
                "--slot-time-ms" => parsed.slot_time_ms = parse_next(&mut args, &arg)?,
                "--flow-control" => parsed.flow_control = true,
                "--send-test-kiss-frame" => {
                    parsed.test_payload = Some(Vec::new());
                }
                "--test-payload-hex" => {
                    parsed.test_payload = Some(parse_hex(next_value(&mut args, &arg)?.as_str())?);
                }
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
        if parsed.command_timeout_ms == 0 {
            return Err("--command-timeout-ms must be > 0".to_string());
        }
        if !(64..=65_535).contains(&parsed.mtu) {
            return Err("--mtu must be between 64 and 65535".to_string());
        }
        Ok(parsed)
    }
}

fn next_value<I>(args: &mut I, flag: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    args.next().ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_next<I, T>(args: &mut I, flag: &str) -> Result<T, String>
where
    I: Iterator<Item = String>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    next_value(args, flag)?.parse::<T>().map_err(|err| format!("{flag} has invalid value: {err}"))
}

fn parse_hex(value: &str) -> Result<Vec<u8>, String> {
    let normalized = value.trim().replace([' ', ':', '-'], "");
    if normalized.len() % 2 != 0 {
        return Err("--test-payload-hex must contain an even number of hex digits".to_string());
    }
    hex::decode(normalized).map_err(|err| format!("--test-payload-hex is invalid hex: {err}"))
}

fn print_usage() {
    println!(
        "Usage: cargo run -p reticulum-rs-transport --features vrn76-kiss-ble --example vrn76_kiss_ble_probe -- --peripheral-id <name|address|id> [options]\n\n\
Options:\n  \
--adapter <name|id>              Match a specific host BLE adapter\n  \
--scan-timeout-ms <ms>           Scan timeout, default 10000\n  \
--command-timeout-ms <ms>        Connect/command timeout, default 3000\n  \
--mtu <bytes>                    KISS payload MTU, default 564\n  \
--preamble-ms <ms>               KISS TXDELAY source value, default 350\n  \
--tx-tail-ms <ms>                KISS TXTAIL source value, default 20\n  \
--persistence <0-255>            KISS persistence, default 64\n  \
--slot-time-ms <ms>              KISS slot time source value, default 20\n  \
--flow-control                   Enable KISS READY flow control\n  \
--send-test-kiss-frame           Send an empty explicit test KISS data frame\n  \
--test-payload-hex <hex>         Send an explicit test KISS data frame with payload"
    );
}
