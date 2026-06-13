#[derive(Parser, Debug)]
#[command(name = "rnx")]
struct Cli {
    #[arg(long)]
    config: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    E2e {
        #[arg(long, default_value_t = 4243)]
        a_port: u16,
        #[arg(long, default_value_t = 4244)]
        b_port: u16,
        #[arg(long, default_value_t = 60)]
        timeout_secs: u64,
        #[arg(long, default_value_t = false)]
        keep: bool,
        #[arg(long = "mode", value_enum)]
        modes: Vec<DeliveryMode>,
    },
    ResourceRepro {
        #[arg(long, default_value_t = 4243)]
        a_port: u16,
        #[arg(long, default_value_t = 4244)]
        b_port: u16,
        #[arg(long, default_value = "134.122.46.48")]
        server_host: String,
        #[arg(long, default_value_t = 37428)]
        server_port: u16,
        #[arg(long, default_value_t = 90)]
        timeout_secs: u64,
        #[arg(long, default_value_t = 4096)]
        large_bytes: usize,
        #[arg(long, default_value_t = false)]
        keep: bool,
    },
    MeshSim {
        #[arg(long, default_value_t = 5)]
        nodes: usize,
        #[arg(long, default_value_t = 4340)]
        base_rpc_port: u16,
        #[arg(long, default_value_t = 90)]
        timeout_secs: u64,
        #[arg(long, default_value_t = false)]
        keep: bool,
        #[arg(long = "mode", value_enum)]
        modes: Vec<DeliveryMode>,
    },
    Replay {
        #[arg(long)]
        trace: PathBuf,
        #[arg(long)]
        capture_out: Option<PathBuf>,
        #[arg(long, default_value = "replay-identity")]
        identity_hash: String,
    },
    CameraUpload {
        #[arg(long, default_value = "127.0.0.1:4243")]
        rpc: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "image/jpeg")]
        content_type: String,
        #[arg(long, default_value_t = 8192)]
        chunk_size: usize,
    },
    CameraCaptureUpload {
        #[arg(long, default_value = "127.0.0.1:4243")]
        rpc: String,
        #[arg(long)]
        peripheral_id: String,
        #[arg(long)]
        service_uuid: String,
        #[arg(long)]
        write_char_uuid: String,
        #[arg(long)]
        notify_char_uuid: String,
        #[arg(long, default_value = "image/jpeg")]
        content_type: String,
        #[arg(long, default_value_t = 8192)]
        chunk_size: usize,
        #[arg(long, default_value_t = 20)]
        timeout_secs: u64,
    },
    BleScan {
        #[arg(long, default_value_t = 10)]
        timeout_secs: u64,
        #[arg(long, default_value_t = 0)]
        limit: usize,
        #[arg(long)]
        service_uuid: Option<String>,
        #[arg(long)]
        manufacturer_prefix: Option<String>,
    },
    BleFindCamera {
        #[arg(long, default_value_t = 12)]
        scan_secs: u64,
        #[arg(long, default_value = "LXMF")]
        name_hint: String,
        #[arg(long)]
        service_uuid: String,
        #[arg(long)]
        write_char_uuid: String,
        #[arg(long)]
        notify_char_uuid: String,
    },
    BleNativePeer {
        #[arg(long, default_value_t = 12)]
        scan_secs: u64,
        #[arg(long, default_value = "LXMF")]
        name_hint: String,
        #[arg(long)]
        peripheral_id: Option<String>,
        #[arg(long)]
        service_uuid: String,
        #[arg(long)]
        write_char_uuid: String,
        #[arg(long)]
        notify_char_uuid: String,
        #[arg(long, value_enum, default_value_t = NativePeerMode::LxmfPing)]
        mode: NativePeerMode,
        #[arg(long)]
        runtime_seq: Option<u32>,
        #[arg(long, default_value = "ping")]
        payload: String,
        #[arg(long, default_value = "22222222222222222222222222222222")]
        destination_hex: String,
        #[arg(long, default_value = "99999999999999999999999999999999")]
        source_hex: String,
        #[arg(long, default_value_t = 8)]
        timeout_secs: u64,
    },
    BleNativeBridge {
        #[arg(long, default_value_t = 12)]
        scan_secs: u64,
        #[arg(long, default_value = "LXMF")]
        name_hint: String,
        #[arg(long)]
        peripheral_id: Option<String>,
        #[arg(long)]
        service_uuid: String,
        #[arg(long)]
        write_char_uuid: String,
        #[arg(long)]
        notify_char_uuid: String,
        #[arg(long, default_value = "127.0.0.1:4243")]
        rpc: String,
        #[arg(long)]
        runtime_seq: Option<u32>,
        #[arg(long, default_value = "bridge-ping")]
        payload: String,
        #[arg(long, default_value = "22222222222222222222222222222222")]
        destination_hex: String,
        #[arg(long, default_value = "99999999999999999999999999999999")]
        source_hex: String,
        #[arg(long, default_value_t = 8)]
        timeout_secs: u64,
        #[arg(long, default_value = "text/plain")]
        content_type: String,
    },
    TcpNativePeer {
        #[arg(long)]
        addr: String,
        #[arg(long, value_enum, default_value_t = NativePeerMode::LxmfPing)]
        mode: NativePeerMode,
        #[arg(long)]
        runtime_seq: Option<u32>,
        #[arg(long, default_value = "ping")]
        payload: String,
        #[arg(long, default_value = "22222222222222222222222222222222")]
        destination_hex: String,
        #[arg(long, default_value = "99999999999999999999999999999999")]
        source_hex: String,
        #[arg(long, default_value_t = 8)]
        timeout_secs: u64,
    },
    TcpNativeListener {
        #[arg(long, default_value = "0.0.0.0:7443")]
        bind: String,
        #[arg(long, default_value_t = false)]
        serve: bool,
        #[arg(long, value_enum, default_value_t = NativeListenerMode::Passive)]
        mode: NativeListenerMode,
        #[arg(long)]
        runtime_seq: Option<u32>,
        #[arg(long, default_value = "ping")]
        payload: String,
        #[arg(long, default_value = "22222222222222222222222222222222")]
        destination_hex: String,
        #[arg(long, default_value = "99999999999999999999999999999999")]
        source_hex: String,
        #[arg(long)]
        capture_out: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = CaptureProfileArg::Default)]
        capture_profile: CaptureProfileArg,
        #[arg(long, default_value_t = 15)]
        timeout_secs: u64,
    },
    TcpNativeBridge {
        #[arg(long, default_value = "0.0.0.0:7443")]
        bind: String,
        #[arg(long, default_value_t = false)]
        serve: bool,
        #[arg(long, value_enum, default_value_t = TcpBridgeMode::Capture)]
        mode: TcpBridgeMode,
        #[arg(long)]
        runtime_seq: Option<u32>,
        #[arg(long, default_value = "bridge-ping")]
        payload: String,
        #[arg(long, default_value = "22222222222222222222222222222222")]
        destination_hex: String,
        #[arg(long, default_value = "99999999999999999999999999999999")]
        source_hex: String,
        #[arg(long, default_value = "127.0.0.1:4243")]
        rpc: String,
        #[arg(long, default_value = "image/jpeg")]
        content_type: String,
        #[arg(long)]
        capture_out: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = CaptureProfileArg::Default)]
        capture_profile: CaptureProfileArg,
        #[arg(long, default_value_t = 8192)]
        chunk_size: usize,
        #[arg(long, default_value_t = 30)]
        timeout_secs: u64,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Hash)]
enum DeliveryMode {
    Direct,
    Opportunistic,
    Propagated,
    Paper,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum NativePeerMode {
    RawPing,
    LxmfPing,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CaptureProfileArg {
    Default,
    Thumbnail,
    Balanced,
    High,
    VeryHigh,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum NativeListenerMode {
    Passive,
    RawPing,
    LxmfPing,
    Capture,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum TcpBridgeMode {
    LxmfPing,
    Capture,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    if let Err(err) = run(cli) {
        log::error!("rnx error: {}", err);
        std::process::exit(1);
    }
}
