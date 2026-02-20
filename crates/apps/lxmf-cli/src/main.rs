#![allow(clippy::result_large_err)]

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use lxmf_sdk::{
    error_code, AuthMode, BindMode, Client, ConfigPatch, ErrorCategory, EventCursor, LxmfSdk,
    LxmfSdkManualTick, MessageId, OverflowPolicy, RpcBackendClient, SdkConfig, SdkError,
    SendRequest, ShutdownMode, StartRequest, TickBudget,
};
use serde_json::{json, Value as JsonValue};
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "lxmf", about = "LXMF operator CLI", version)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:4242")]
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
    },
    Cancel {
        #[arg(long)]
        message_id: String,
    },
    Status {
        #[arg(long)]
        message_id: String,
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
    let cli = Cli::parse();
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

fn build_start_request(cli: &Cli) -> Result<StartRequest, SdkError> {
    let mut config = match cli.profile {
        ProfileArg::DesktopFull => SdkConfig::desktop_full_default(),
        ProfileArg::DesktopLocalRuntime => SdkConfig::desktop_local_default(),
        ProfileArg::EmbeddedAlloc => SdkConfig::embedded_alloc_default(),
    }
    .with_rpc_listen_addr(cli.rpc.clone());
    config.bind_mode = bind_mode_value(cli.bind_mode);
    config.auth_mode = auth_mode_value(cli.auth_mode);
    config.overflow_policy = overflow_policy_value(cli.overflow_policy);
    config.block_timeout_ms = cli.block_timeout_ms;
    config.event_stream.max_poll_events = cli.max_poll_events;
    config.event_stream.max_event_bytes = cli.max_event_bytes;
    config.event_stream.max_batch_bytes = cli.max_batch_bytes;
    config.event_stream.max_extension_keys = cli.max_extension_keys;
    config.idempotency_ttl_ms = cli.idempotency_ttl_ms;
    if let Some(backend) = config.rpc_backend.as_mut() {
        backend.listen_addr = cli.rpc.clone();
        backend.read_timeout_ms = cli.read_timeout_ms;
        backend.write_timeout_ms = cli.write_timeout_ms;
        backend.max_header_bytes = cli.max_header_bytes;
        backend.max_body_bytes = cli.max_body_bytes;
    }

    match config.auth_mode {
        AuthMode::Token => {
            let issuer = required_string(
                cli.token_issuer.as_deref(),
                "--token-issuer is required in token auth mode",
            )?;
            let audience = required_string(
                cli.token_audience.as_deref(),
                "--token-audience is required in token auth mode",
            )?;
            let secret = required_string(
                cli.token_shared_secret.as_deref(),
                "--token-shared-secret is required in token auth mode",
            )?;
            config = config.with_token_auth(issuer, audience, secret);
            if let Some(backend) = config.rpc_backend.as_mut() {
                if let Some(token_auth) = backend.token_auth.as_mut() {
                    token_auth.jti_cache_ttl_ms = cli.token_jti_cache_ttl_ms;
                    token_auth.clock_skew_ms = cli.token_clock_skew_ms;
                }
                backend.listen_addr = cli.rpc.clone();
                backend.read_timeout_ms = cli.read_timeout_ms;
                backend.write_timeout_ms = cli.write_timeout_ms;
                backend.max_header_bytes = cli.max_header_bytes;
                backend.max_body_bytes = cli.max_body_bytes;
            }
        }
        AuthMode::Mtls => {
            let ca_bundle_path = required_string(
                cli.mtls_ca_bundle_path.as_deref(),
                "--mtls-ca-bundle-path is required in mtls auth mode",
            )?;
            config = config.with_mtls_auth(ca_bundle_path);
            if let Some(backend) = config.rpc_backend.as_mut() {
                if let Some(mtls_auth) = backend.mtls_auth.as_mut() {
                    mtls_auth.require_client_cert = cli.mtls_require_client_cert;
                    mtls_auth.allowed_san = cli
                        .mtls_allowed_san
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned);
                }
                backend.listen_addr = cli.rpc.clone();
                backend.read_timeout_ms = cli.read_timeout_ms;
                backend.write_timeout_ms = cli.write_timeout_ms;
                backend.max_header_bytes = cli.max_header_bytes;
                backend.max_body_bytes = cli.max_body_bytes;
            }
        }
        AuthMode::LocalTrusted => {}
        _ => {
            return Err(invalid_argument("unsupported auth mode for this CLI build"));
        }
    }

    let request = StartRequest::new(config)
        .with_supported_contract_versions(if cli.contract_versions.is_empty() {
            vec![2]
        } else {
            cli.contract_versions.clone()
        })
        .with_requested_capabilities(cli.requested_capabilities.clone());
    request.validate()?;
    Ok(request)
}

fn required_string(value: Option<&str>, missing_msg: &str) -> Result<String, SdkError> {
    let value = value.map(str::trim).unwrap_or_default();
    if value.is_empty() {
        return Err(invalid_argument(missing_msg));
    }
    Ok(value.to_owned())
}

fn invalid_argument(message: impl Into<String>) -> SdkError {
    SdkError::new(error_code::VALIDATION_INVALID_ARGUMENT, ErrorCategory::Validation, message)
        .with_user_actionable(true)
}

fn bind_mode_value(bind_mode: BindModeArg) -> BindMode {
    match bind_mode {
        BindModeArg::LocalOnly => BindMode::LocalOnly,
        BindModeArg::Remote => BindMode::Remote,
    }
}

fn auth_mode_value(auth_mode: AuthModeArg) -> AuthMode {
    match auth_mode {
        AuthModeArg::LocalTrusted => AuthMode::LocalTrusted,
        AuthModeArg::Token => AuthMode::Token,
        AuthModeArg::Mtls => AuthMode::Mtls,
    }
}

fn overflow_policy_value(policy: OverflowPolicyArg) -> OverflowPolicy {
    match policy {
        OverflowPolicyArg::Reject => OverflowPolicy::Reject,
        OverflowPolicyArg::DropOldest => OverflowPolicy::DropOldest,
        OverflowPolicyArg::Block => OverflowPolicy::Block,
    }
}

fn output_mode(cli: &Cli) -> OutputModeArg {
    if cli.json {
        OutputModeArg::JsonPretty
    } else {
        cli.output
    }
}

fn completion_shell_name(shell: CompletionShellArg) -> &'static str {
    match shell {
        CompletionShellArg::Bash => "bash",
        CompletionShellArg::Zsh => "zsh",
        CompletionShellArg::Fish => "fish",
        CompletionShellArg::PowerShell => "powershell",
        CompletionShellArg::Elvish => "elvish",
    }
}

fn to_completion_shell(shell: CompletionShellArg) -> Shell {
    match shell {
        CompletionShellArg::Bash => Shell::Bash,
        CompletionShellArg::Zsh => Shell::Zsh,
        CompletionShellArg::Fish => Shell::Fish,
        CompletionShellArg::PowerShell => Shell::PowerShell,
        CompletionShellArg::Elvish => Shell::Elvish,
    }
}

fn generate_completions(shell: CompletionShellArg) -> String {
    let mut command = Cli::command();
    let mut buffer = Vec::new();
    generate(to_completion_shell(shell), &mut command, "lxmf", &mut buffer);
    String::from_utf8_lossy(&buffer).into_owned()
}

fn emit_json_envelope(value: JsonValue, pretty: bool) {
    let envelope = json!({
        "ok": true,
        "result": value,
    });
    let serialized = if pretty {
        serde_json::to_string_pretty(&envelope)
    } else {
        serde_json::to_string(&envelope)
    };
    match serialized {
        Ok(serialized) => println!("{serialized}"),
        Err(err) => {
            println!("{{\"ok\":true,\"result\":null,\"warning\":\"serialization failed: {err}\"}}")
        }
    }
}

fn emit_human_output(cli: &Cli, value: &JsonValue) {
    match &cli.command {
        Command::Start => {
            println!("runtime started");
            if let Some(runtime) = value.get("runtime").and_then(JsonValue::as_object) {
                if let Some(runtime_id) = runtime.get("runtime_id").and_then(JsonValue::as_str) {
                    println!("runtime_id: {runtime_id}");
                }
                if let Some(contract) =
                    runtime.get("active_contract_version").and_then(JsonValue::as_u64)
                {
                    println!("contract_version: {contract}");
                }
            }
        }
        Command::Send { .. } => {
            if let Some(message_id) = value.get("message_id").and_then(JsonValue::as_str) {
                println!("message queued: {message_id}");
            } else {
                println!("{value}");
            }
        }
        Command::Cancel { .. } => {
            if let Some(result) = value.get("result") {
                println!("cancel result: {result}");
            } else {
                println!("{value}");
            }
        }
        Command::Status { .. } => {
            if let Some(message) = value.get("message") {
                println!("message status: {message}");
            } else {
                println!("{value}");
            }
        }
        Command::Poll { .. } => {
            let count = value
                .get("events")
                .and_then(JsonValue::as_array)
                .map(|events| events.len())
                .unwrap_or(0);
            println!("events: {count}");
            if let Some(cursor) = value.get("next_cursor").and_then(JsonValue::as_str) {
                println!("next_cursor: {cursor}");
            }
            if let Some(dropped) = value.get("dropped_count").and_then(JsonValue::as_u64) {
                println!("dropped_count: {dropped}");
            }
        }
        Command::Snapshot => {
            if let Some(runtime) = value.get("runtime") {
                println!("runtime snapshot: {runtime}");
            } else {
                println!("{value}");
            }
        }
        Command::Configure { .. } => {
            if let Some(ack) = value.get("ack") {
                println!("configure result: {ack}");
            } else {
                println!("{value}");
            }
        }
        Command::Shutdown { .. } => {
            if let Some(ack) = value.get("ack") {
                println!("shutdown result: {ack}");
            } else {
                println!("{value}");
            }
        }
        Command::Tick { .. } => {
            if let Some(tick) = value.get("tick") {
                println!("tick result: {tick}");
            } else {
                println!("{value}");
            }
        }
        Command::Completions { .. } => {
            if let Some(script) = value.get("script").and_then(JsonValue::as_str) {
                print!("{script}");
            }
        }
    }
}

fn emit_output(cli: &Cli, value: JsonValue) {
    if cli.quiet {
        return;
    }

    match output_mode(cli) {
        OutputModeArg::Json => emit_json_envelope(value, false),
        OutputModeArg::JsonPretty => emit_json_envelope(value, true),
        OutputModeArg::Human => emit_human_output(cli, &value),
    }
}

fn emit_error(cli: &Cli, err: SdkError) {
    match output_mode(cli) {
        OutputModeArg::Human => {}
        OutputModeArg::Json | OutputModeArg::JsonPretty => {
            let machine_code = err.machine_code.clone();
            let message = err.message.clone();
            let envelope = json!({
                "ok": false,
                "error": err,
            });
            let serialized = match output_mode(cli) {
                OutputModeArg::Json => serde_json::to_string(&envelope),
                OutputModeArg::JsonPretty | OutputModeArg::Human => {
                    serde_json::to_string_pretty(&envelope)
                }
            };
            match serialized {
                Ok(serialized) => eprintln!("{serialized}"),
                Err(ser_err) => {
                    eprintln!(
                    "{{\"ok\":false,\"error\":{{\"machine_code\":\"{}\",\"message\":\"{}\",\"serialization\":\"{}\"}}}}",
                        machine_code, message, ser_err
                );
                }
            }
            return;
        }
    }

    eprintln!("error [{}]: {}", err.machine_code, err.message);
    if !err.details.is_empty() {
        eprintln!("details: {}", JsonValue::Object(err.details.into_iter().collect()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("cli args should parse")
    }

    #[test]
    fn payload_requires_content_when_payload_json_missing() {
        let err = build_payload(None, None, None).expect_err("missing content should fail");
        assert_eq!(err.machine_code, error_code::VALIDATION_INVALID_ARGUMENT);
    }

    #[test]
    fn payload_json_cannot_be_combined_with_content_flags() {
        let err = build_payload(Some("hello"), None, Some("{\"content\":\"x\"}"))
            .expect_err("payload_json + content should fail");
        assert_eq!(err.machine_code, error_code::VALIDATION_INVALID_ARGUMENT);
    }

    #[test]
    fn start_request_defaults_are_valid() {
        let cli = parse_cli(&["lxmf-cli", "start"]);
        let request = build_start_request(&cli).expect("default start request should be valid");
        assert_eq!(request.supported_contract_versions, vec![2]);
    }

    #[test]
    fn token_auth_mode_requires_shared_secret() {
        let cli = parse_cli(&[
            "lxmf-cli",
            "--bind-mode",
            "remote",
            "--auth-mode",
            "token",
            "--token-issuer",
            "issuer-a",
            "--token-audience",
            "aud-a",
            "start",
        ]);
        let err = build_start_request(&cli).expect_err("missing token secret should fail");
        assert_eq!(err.machine_code, error_code::VALIDATION_INVALID_ARGUMENT);
    }

    #[test]
    fn output_mode_defaults_to_human() {
        let cli = parse_cli(&["lxmf-cli", "start"]);
        assert_eq!(output_mode(&cli), OutputModeArg::Human);
    }

    #[test]
    fn legacy_json_flag_maps_to_json_pretty_output() {
        let cli = parse_cli(&["lxmf-cli", "--json", "start"]);
        assert_eq!(output_mode(&cli), OutputModeArg::JsonPretty);
    }

    #[test]
    fn completions_command_generates_nonempty_script() {
        let cli = parse_cli(&["lxmf-cli", "completions", "--shell", "bash"]);
        let output = run(&cli).expect("completion generation should succeed");
        let script = output
            .get("script")
            .and_then(JsonValue::as_str)
            .expect("completion payload should contain script");
        assert!(script.contains("lxmf"));
        assert!(!script.trim().is_empty());
    }
}
