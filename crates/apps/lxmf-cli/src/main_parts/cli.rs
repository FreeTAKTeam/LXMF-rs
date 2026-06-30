#[derive(Parser, Debug)]
#[command(name = "lxmf", about = "LXMF operator CLI", disable_version_flag = true)]
struct Cli {
    #[arg(long, default_value = "unix:/tmp/lxmf-rpc.sock")]
    rpc: String,

    #[arg(long, value_enum, default_value_t = ProfileArg::DesktopFull)]
    profile: ProfileArg,

    #[arg(long, value_enum, default_value_t = BindModeArg::LocalOnly)]
    bind_mode: BindModeArg,

    #[arg(long, value_enum, default_value_t = AuthModeArg::LocalTrusted)]
    auth_mode: AuthModeArg,

    #[arg(long, value_enum, default_value_t = OverflowPolicyArg::Reject)]
    overflow_policy: OverflowPolicyArg,

    #[arg(long)]
    block_timeout_ms: Option<u64>,

    #[arg(long = "contract-version")]
    contract_versions: Vec<u16>,

    #[arg(long = "requested-capability")]
    requested_capabilities: Vec<String>,

    #[arg(long, default_value_t = 128)]
    max_poll_events: usize,

    #[arg(long, default_value_t = 32_768)]
    max_event_bytes: usize,

    #[arg(long, default_value_t = 1_048_576)]
    max_batch_bytes: usize,

    #[arg(long, default_value_t = 32)]
    max_extension_keys: usize,

    #[arg(long, default_value_t = 86_400_000)]
    idempotency_ttl_ms: u64,

    #[arg(long, default_value_t = 5_000)]
    read_timeout_ms: u64,

    #[arg(long, default_value_t = 5_000)]
    write_timeout_ms: u64,

    #[arg(long, default_value_t = 16_384)]
    max_header_bytes: usize,

    #[arg(long, default_value_t = 1_048_576)]
    max_body_bytes: usize,

    #[arg(long)]
    token_issuer: Option<String>,

    #[arg(long)]
    token_audience: Option<String>,

    #[arg(long)]
    token_shared_secret: Option<String>,

    #[arg(long, default_value_t = 60_000)]
    token_jti_cache_ttl_ms: u64,

    #[arg(long, default_value_t = 30_000)]
    token_clock_skew_ms: u64,

    #[arg(long)]
    mtls_ca_bundle_path: Option<String>,

    #[arg(long, default_value_t = true)]
    mtls_require_client_cert: bool,

    #[arg(long)]
    mtls_allowed_san: Option<String>,

    #[arg(long)]
    json: bool,

    #[arg(long, value_enum, default_value_t = OutputModeArg::Human)]
    output: OutputModeArg,

    #[arg(long)]
    quiet: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProfileArg {
    #[value(name = "desktop-full")]
    DesktopFull,
    #[value(name = "desktop-local-runtime")]
    DesktopLocalRuntime,
    #[value(name = "embedded-alloc")]
    EmbeddedAlloc,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BindModeArg {
    #[value(name = "local_only")]
    LocalOnly,
    #[value(name = "remote")]
    Remote,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AuthModeArg {
    #[value(name = "local_trusted")]
    LocalTrusted,
    #[value(name = "token")]
    Token,
    #[value(name = "mtls")]
    Mtls,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OverflowPolicyArg {
    #[value(name = "reject")]
    Reject,
    #[value(name = "drop_oldest")]
    DropOldest,
    #[value(name = "block")]
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum OutputModeArg {
    #[value(name = "human")]
    Human,
    #[value(name = "json")]
    Json,
    #[value(name = "json-pretty")]
    JsonPretty,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ShutdownModeArg {
    #[value(name = "graceful")]
    Graceful,
    #[value(name = "immediate")]
    Immediate,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CompletionShellArg {
    #[value(name = "bash")]
    Bash,
    #[value(name = "zsh")]
    Zsh,
    #[value(name = "fish")]
    Fish,
    #[value(name = "powershell")]
    PowerShell,
    #[value(name = "elvish")]
    Elvish,
}

#[derive(Subcommand, Debug)]
enum Command {
    Start,
    Send {
        #[arg(long)]
        source: String,
        #[arg(long)]
        destination: String,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        payload_json: Option<String>,
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        ttl_ms: Option<u64>,
        #[arg(long)]
        correlation_id: Option<String>,
        #[arg(long)]
        delivery_method: Option<String>,
        #[arg(long)]
        stamp_cost: Option<u32>,
        #[arg(long)]
        include_ticket: bool,
        #[arg(long)]
        try_propagation_on_fail: bool,
    },
    Cancel {
        #[arg(long)]
        message_id: String,
    },
    Status {
        #[arg(long)]
        message_id: String,
    },
    PaperEncode {
        #[arg(long)]
        message_id: String,
    },
    PaperDecode {
        #[arg(long)]
        uri: String,
        #[arg(long)]
        transient_id: Option<String>,
        #[arg(long)]
        destination_hint: Option<String>,
    },
    Poll {
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, default_value_t = 64)]
        max: usize,
    },
    Snapshot,
    Configure {
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        patch_json: String,
    },
    Shutdown {
        #[arg(long, value_enum, default_value_t = ShutdownModeArg::Graceful)]
        mode: ShutdownModeArg,
    },
    Tick {
        #[arg(long, default_value_t = 128)]
        max_work_items: usize,
        #[arg(long)]
        max_duration_ms: Option<u64>,
    },
    Completions {
        #[arg(long, value_enum)]
        shell: CompletionShellArg,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = version::parse_with_version::<Cli>();
    match run(&cli) {
        Ok(output) => {
            emit_output(&cli, output);
            ExitCode::SUCCESS
        }
        Err(err) => {
            emit_error(&cli, err);
            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli) -> Result<JsonValue, SdkError> {
    if let Command::Completions { shell } = &cli.command {
        return Ok(json!({
            "shell": completion_shell_name(*shell),
            "script": generate_completions(*shell),
        }));
    }

    let backend = RpcBackendClient::new(cli.rpc.clone());
    let client = Client::new(backend);

    match &cli.command {
        Command::Start => {
            let handle = client.start(build_start_request(cli)?)?;
            Ok(json!({ "runtime": handle }))
        }
        Command::Send {
            source,
            destination,
            content,
            title,
            payload_json,
            idempotency_key,
            ttl_ms,
            correlation_id,
            delivery_method,
            stamp_cost,
            include_ticket,
            try_propagation_on_fail,
        } => {
            ensure_started(&client, cli)?;
            let payload =
                build_payload(content.as_deref(), title.as_deref(), payload_json.as_deref())?;
            let mut req = SendRequest::new(source.clone(), destination.clone(), payload);
            if let Some(key) = idempotency_key.clone() {
                req = req.with_idempotency_key(key);
            }
            if let Some(ttl_ms) = ttl_ms {
                req = req.with_ttl_ms(*ttl_ms);
            }
            if let Some(correlation_id) = correlation_id.clone() {
                req = req.with_correlation_id(correlation_id);
            }
            if let Some(delivery_method) = delivery_method.clone() {
                req = req.with_delivery_method(delivery_method);
            }
            if let Some(stamp_cost) = stamp_cost {
                req = req.with_stamp_cost(*stamp_cost);
            }
            if *include_ticket {
                req = req.with_include_ticket(true);
            }
            if *try_propagation_on_fail {
                req = req.with_try_propagation_on_fail(true);
            }
            let message_id = client.send(req)?;
            Ok(json!({ "message_id": message_id }))
        }
        Command::Cancel { message_id } => {
            ensure_started(&client, cli)?;
            let result = client.cancel(MessageId(message_id.clone()))?;
            Ok(json!({ "result": result }))
        }
        Command::Status { message_id } => {
            ensure_started(&client, cli)?;
            let snapshot = client.status(MessageId(message_id.clone()))?;
            Ok(json!({ "message": snapshot }))
        }
        Command::PaperEncode { message_id } => {
            ensure_started(&client, cli)?;
            let envelope = client.paper_encode(MessageId(message_id.clone()))?;
            Ok(json!({ "envelope": envelope }))
        }
        Command::PaperDecode {
            uri,
            transient_id,
            destination_hint,
        } => {
            ensure_started(&client, cli)?;
            let envelope = build_paper_decode_envelope(uri, transient_id.clone(), destination_hint.clone())?;
            let result = client.paper_decode_with_metadata(envelope)?;
            Ok(json!({ "paper": result }))
        }
        Command::Poll { cursor, max } => {
            ensure_started(&client, cli)?;
            let batch = client.poll_events(cursor.clone().map(EventCursor), *max)?;
            Ok(json!({
                "events": batch.events,
                "next_cursor": batch.next_cursor,
                "dropped_count": batch.dropped_count,
                "snapshot_high_watermark_seq_no": batch.snapshot_high_watermark_seq_no
            }))
        }
        Command::Snapshot => {
            ensure_started(&client, cli)?;
            let snapshot = client.snapshot()?;
            Ok(json!({ "runtime": snapshot }))
        }
        Command::Configure { expected_revision, patch_json } => {
            ensure_started(&client, cli)?;
            let patch: ConfigPatch = serde_json::from_str(patch_json).map_err(|err| {
                invalid_argument(format!("patch_json must be valid ConfigPatch JSON: {err}"))
            })?;
            let ack = client.configure(*expected_revision, patch)?;
            Ok(json!({ "ack": ack }))
        }
        Command::Shutdown { mode } => {
            ensure_started(&client, cli)?;
            let shutdown_mode = match mode {
                ShutdownModeArg::Graceful => ShutdownMode::Graceful,
                ShutdownModeArg::Immediate => ShutdownMode::Immediate,
            };
            let ack = client.shutdown(shutdown_mode)?;
            Ok(json!({ "ack": ack }))
        }
        Command::Tick { max_work_items, max_duration_ms } => {
            ensure_started(&client, cli)?;
            let mut budget = TickBudget::new(*max_work_items);
            if let Some(max_duration_ms) = max_duration_ms {
                budget = budget.with_max_duration_ms(*max_duration_ms);
            }
            let result = client.tick(budget)?;
            Ok(json!({ "tick": result }))
        }
        Command::Completions { .. } => unreachable!("handled before backend bootstrap"),
    }
}

fn ensure_started(client: &Client<RpcBackendClient>, cli: &Cli) -> Result<(), SdkError> {
    let _ = client.start(build_start_request(cli)?)?;
    Ok(())
}

fn build_payload(
    content: Option<&str>,
    title: Option<&str>,
    payload_json: Option<&str>,
) -> Result<JsonValue, SdkError> {
    if let Some(raw) = payload_json {
        if content.is_some() || title.is_some() {
            return Err(invalid_argument(
                "payload_json cannot be combined with content/title flags",
            ));
        }
        return serde_json::from_str(raw)
            .map_err(|err| invalid_argument(format!("payload_json is not valid JSON: {err}")));
    }

    let content = content.unwrap_or("").trim().to_owned();
    if content.is_empty() {
        return Err(invalid_argument("content is required when payload_json is not provided"));
    }

    Ok(json!({
        "content": content,
        "title": title.unwrap_or_default(),
    }))
}

fn build_paper_decode_envelope(
    uri: &str,
    transient_id: Option<String>,
    destination_hint: Option<String>,
) -> Result<PaperMessageEnvelope, SdkError> {
    let uri = uri.trim();
    if !uri.starts_with("lxm://") {
        return Err(invalid_argument("paper URI must start with lxm://"));
    }
    Ok(PaperMessageEnvelope {
        uri: uri.to_owned(),
        transient_id,
        destination_hint,
        extensions: Default::default(),
    })
}
