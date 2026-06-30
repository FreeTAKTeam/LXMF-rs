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
        Command::PaperEncode { .. } => {
            if let Some(envelope) = value.get("envelope") {
                println!("{envelope}");
            } else {
                println!("{value}");
            }
        }
        Command::PaperDecode { .. } => {
            if let Some(paper) = value.get("paper") {
                println!("paper decode result: {paper}");
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
            eprintln!("{}", serialize_error_for_stderr(cli, &err));
            return;
        }
    }

    log::error!("error [{}]: {}", err.machine_code, err.message);
    if !err.details.is_empty() {
        log::error!("details: {}", JsonValue::Object(err.details.into_iter().collect()));
    }
}

fn serialize_error_for_stderr(cli: &Cli, err: &SdkError) -> String {
    let envelope = json!({
        "ok": false,
        "error": err,
    });
    let serialized = match output_mode(cli) {
        OutputModeArg::Json => serde_json::to_string(&envelope),
        OutputModeArg::JsonPretty | OutputModeArg::Human => serde_json::to_string_pretty(&envelope),
    };
    serialized.unwrap_or_else(|ser_err| {
        let fallback = json!({
            "ok": false,
            "error": {
                "machine_code": err.machine_code,
                "message": err.message,
                "serialization": ser_err.to_string(),
            },
        });
        serde_json::to_string(&fallback).unwrap_or_else(|_| {
            "{\"ok\":false,\"error\":{\"machine_code\":\"serialization_failed\"}}".to_string()
        })
    })
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
        assert_eq!(cli.rpc, "unix:/tmp/lxmf-rpc.sock");
        let request = build_start_request(&cli).expect("default start request should be valid");
        assert_eq!(request.supported_contract_versions, vec![2]);
    }

    #[test]
    fn send_command_accepts_delivery_option_flags() {
        let cli = parse_cli(&[
            "lxmf-cli",
            "send",
            "--source",
            "source.peer",
            "--destination",
            "dest.peer",
            "--content",
            "hello",
            "--delivery-method",
            "propagated",
            "--stamp-cost",
            "4",
            "--include-ticket",
            "--try-propagation-on-fail",
        ]);

        let Command::Send {
            delivery_method,
            stamp_cost,
            include_ticket,
            try_propagation_on_fail,
            ..
        } = cli.command
        else {
            panic!("expected send command");
        };

        assert_eq!(delivery_method.as_deref(), Some("propagated"));
        assert_eq!(stamp_cost, Some(4));
        assert!(include_ticket);
        assert!(try_propagation_on_fail);
    }

    #[test]
    fn paper_encode_command_accepts_message_id() {
        let cli = parse_cli(&["lxmf-cli", "paper-encode", "--message-id", "msg-1"]);

        let Command::PaperEncode { message_id } = cli.command else {
            panic!("expected paper encode command");
        };

        assert_eq!(message_id, "msg-1");
    }

    #[test]
    fn paper_decode_command_builds_envelope_from_uri() {
        let cli = parse_cli(&[
            "lxmf-cli",
            "paper-decode",
            "--uri",
            "lxm://paper/v1/abc",
            "--transient-id",
            "paper-1",
            "--destination-hint",
            "dest",
        ]);

        let Command::PaperDecode {
            uri,
            transient_id,
            destination_hint,
        } = cli.command
        else {
            panic!("expected paper decode command");
        };
        let envelope = build_paper_decode_envelope(&uri, transient_id, destination_hint)
            .expect("paper envelope should build");

        assert_eq!(envelope.uri, "lxm://paper/v1/abc");
        assert_eq!(envelope.transient_id.as_deref(), Some("paper-1"));
        assert_eq!(envelope.destination_hint.as_deref(), Some("dest"));
        assert!(envelope.extensions.is_empty());
    }

    #[test]
    fn paper_decode_envelope_rejects_non_lxm_uri() {
        let err = build_paper_decode_envelope("not-a-paper-uri", None, None)
            .expect_err("invalid paper URI should fail");

        assert_eq!(err.machine_code, error_code::VALIDATION_INVALID_ARGUMENT);
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
    fn json_error_output_is_raw_json_for_stderr() {
        let cli = parse_cli(&["lxmf-cli", "--output", "json", "start"]);
        let err = invalid_argument("missing --config");

        let output = serialize_error_for_stderr(&cli, &err);

        let parsed: JsonValue = serde_json::from_str(&output).expect("error output should be JSON");
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["error"]["machine_code"], error_code::VALIDATION_INVALID_ARGUMENT);
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
