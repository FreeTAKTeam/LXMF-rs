#![allow(clippy::result_large_err)]

use clap::{Parser, Subcommand, ValueEnum};
use lxmf_sdk::{
    error_code, Client, ConfigPatch, ErrorCategory, EventCursor, LxmfSdk, LxmfSdkManualTick,
    MessageId, RpcBackendClient, SdkError, ShutdownMode, StartRequest, TickBudget,
};
use serde::de::DeserializeOwned;
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

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ShutdownModeArg {
    #[value(name = "graceful")]
    Graceful,
    #[value(name = "immediate")]
    Immediate,
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
            let req = parse_struct(
                json!({
                    "source": source,
                    "destination": destination,
                    "payload": payload,
                    "idempotency_key": idempotency_key,
                    "ttl_ms": ttl_ms,
                    "correlation_id": correlation_id,
                    "extensions": {},
                }),
                "send request",
            )?;
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
            let budget: TickBudget = parse_struct(
                json!({
                    "max_work_items": max_work_items,
                    "max_duration_ms": max_duration_ms,
                }),
                "tick budget",
            )?;
            let result = client.tick(budget)?;
            Ok(json!({ "tick": result }))
        }
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
    let auth_mode = auth_mode_wire(cli.auth_mode);
    let token_auth = if auth_mode == "token" {
        Some(json!({
            "issuer": required_string(
                cli.token_issuer.as_deref(),
                "--token-issuer is required in token auth mode",
            )?,
            "audience": required_string(
                cli.token_audience.as_deref(),
                "--token-audience is required in token auth mode",
            )?,
            "jti_cache_ttl_ms": cli.token_jti_cache_ttl_ms,
            "clock_skew_ms": cli.token_clock_skew_ms,
            "shared_secret": required_string(
                cli.token_shared_secret.as_deref(),
                "--token-shared-secret is required in token auth mode",
            )?,
        }))
    } else {
        None
    };

    let mtls_auth = if auth_mode == "mtls" {
        Some(json!({
            "ca_bundle_path": required_string(
                cli.mtls_ca_bundle_path.as_deref(),
                "--mtls-ca-bundle-path is required in mtls auth mode",
            )?,
            "require_client_cert": cli.mtls_require_client_cert,
            "allowed_san": cli
                .mtls_allowed_san
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        }))
    } else {
        None
    };

    let request: StartRequest = parse_struct(
        json!({
            "supported_contract_versions": if cli.contract_versions.is_empty() { vec![2] } else { cli.contract_versions.clone() },
            "requested_capabilities": cli.requested_capabilities.clone(),
            "config": {
                "profile": profile_wire(cli.profile),
                "bind_mode": bind_mode_wire(cli.bind_mode),
                "auth_mode": auth_mode,
                "overflow_policy": overflow_policy_wire(cli.overflow_policy),
                "block_timeout_ms": cli.block_timeout_ms,
                "event_stream": {
                    "max_poll_events": cli.max_poll_events,
                    "max_event_bytes": cli.max_event_bytes,
                    "max_batch_bytes": cli.max_batch_bytes,
                    "max_extension_keys": cli.max_extension_keys,
                },
                "idempotency_ttl_ms": cli.idempotency_ttl_ms,
                "redaction": {
                    "enabled": true,
                    "sensitive_transform": "hash",
                    "break_glass_allowed": false,
                    "break_glass_ttl_ms": JsonValue::Null,
                },
                "rpc_backend": {
                    "listen_addr": cli.rpc,
                    "read_timeout_ms": cli.read_timeout_ms,
                    "write_timeout_ms": cli.write_timeout_ms,
                    "max_header_bytes": cli.max_header_bytes,
                    "max_body_bytes": cli.max_body_bytes,
                    "token_auth": token_auth,
                    "mtls_auth": mtls_auth,
                },
                "extensions": {},
            },
        }),
        "start request",
    )?;
    request.validate()?;
    Ok(request)
}

fn parse_struct<T: DeserializeOwned>(value: JsonValue, context: &str) -> Result<T, SdkError> {
    serde_json::from_value(value)
        .map_err(|err| invalid_argument(format!("{context} is invalid: {err}")))
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

fn profile_wire(profile: ProfileArg) -> &'static str {
    match profile {
        ProfileArg::DesktopFull => "desktop-full",
        ProfileArg::DesktopLocalRuntime => "desktop-local-runtime",
        ProfileArg::EmbeddedAlloc => "embedded-alloc",
    }
}

fn bind_mode_wire(bind_mode: BindModeArg) -> &'static str {
    match bind_mode {
        BindModeArg::LocalOnly => "local_only",
        BindModeArg::Remote => "remote",
    }
}

fn auth_mode_wire(auth_mode: AuthModeArg) -> &'static str {
    match auth_mode {
        AuthModeArg::LocalTrusted => "local_trusted",
        AuthModeArg::Token => "token",
        AuthModeArg::Mtls => "mtls",
    }
}

fn overflow_policy_wire(policy: OverflowPolicyArg) -> &'static str {
    match policy {
        OverflowPolicyArg::Reject => "reject",
        OverflowPolicyArg::DropOldest => "drop_oldest",
        OverflowPolicyArg::Block => "block",
    }
}

fn emit_output(cli: &Cli, value: JsonValue) {
    if cli.quiet {
        return;
    }
    let envelope = json!({
        "ok": true,
        "result": value,
    });
    if cli.json {
        match serde_json::to_string_pretty(&envelope) {
            Ok(serialized) => println!("{serialized}"),
            Err(err) => println!(
                "{{\"ok\":true,\"result\":null,\"warning\":\"serialization failed: {err}\"}}"
            ),
        }
    } else {
        match serde_json::to_string(&envelope) {
            Ok(serialized) => println!("{serialized}"),
            Err(err) => println!(
                "{{\"ok\":true,\"result\":null,\"warning\":\"serialization failed: {err}\"}}"
            ),
        }
    }
}

fn emit_error(cli: &Cli, err: SdkError) {
    if cli.json {
        let envelope = json!({
            "ok": false,
            "error": err,
        });
        match serde_json::to_string_pretty(&envelope) {
            Ok(serialized) => eprintln!("{serialized}"),
            Err(ser_err) => {
                eprintln!(
                    "{{\"ok\":false,\"error\":{{\"machine_code\":\"{}\",\"message\":\"{}\",\"serialization\":\"{}\"}}}}",
                    err.machine_code, err.message, ser_err
                );
            }
        }
        return;
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
}
