use serde_json::json;
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode};
use std::thread;
use std::time::{Duration, Instant};

use super::{python_compat::apply_python_compat_config, rpc_client};

pub(crate) fn requires_supervised_launch(args: &crate::EffectiveArgs) -> bool {
    args.propagation_node || args.on_inbound.is_some()
}

pub(crate) fn launch_supervised(
    mut cmd: Command,
    reticulumd: PathBuf,
    rpc_addr: &str,
    args: &crate::EffectiveArgs,
) -> ExitCode {
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!("lxmd: failed to launch {}: {}", reticulumd.display(), err);
            return ExitCode::from(1);
        }
    };

    if let Err(err) = wait_until_ready(&mut child, rpc_addr, crate::READY_TIMEOUT) {
        eprintln!("lxmd: {err}");
        let _ = child.kill();
        let _ = child.wait();
        return ExitCode::from(1);
    }

    if args.propagation_node {
        if let Err(err) = enable_propagation_mode(rpc_addr) {
            eprintln!("lxmd: failed to enable propagation mode: {err}");
            let _ = child.kill();
            let _ = child.wait();
            return ExitCode::from(1);
        }
    }

    if let Err(err) = apply_python_compat_config(rpc_addr, args) {
        eprintln!("lxmd: failed to apply python-style daemon settings: {err}");
        let _ = child.kill();
        let _ = child.wait();
        return ExitCode::from(1);
    }

    if args.propagation_node {
        if let Err(err) = rpc_client::rpc_call(rpc_addr, "announce_now", None) {
            eprintln!("lxmd: failed to announce propagation state: {err}");
        }
    }

    if let Some(command) = args.on_inbound.clone() {
        rpc_client::spawn_on_inbound_watcher(
            rpc_addr.to_string(),
            command,
            args.messages_dir.clone(),
        );
    }

    match child.wait() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(err) => {
            eprintln!("lxmd: failed waiting for reticulumd: {}", err);
            ExitCode::from(1)
        }
    }
}

fn wait_until_ready(child: &mut Child, rpc_addr: &str, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(format!("reticulumd exited before becoming ready: {}", status));
            }
            Ok(None) => {}
            Err(err) => return Err(format!("failed to check reticulumd status: {err}")),
        }

        match http_get_ready(rpc_addr) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(_) => {}
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for reticulumd readiness at http://{rpc_addr}/readyz"
            ));
        }
        thread::sleep(crate::READY_POLL_INTERVAL);
    }
}

fn enable_propagation_mode(rpc_addr: &str) -> Result<(), String> {
    let response = rpc_client::rpc_call(
        rpc_addr,
        "propagation_enable",
        Some(json!({
            "enabled": true,
        })),
    )?;
    if let Some(error) = response.get("error").and_then(|value| value.as_object()) {
        let message =
            error.get("message").and_then(|value| value.as_str()).unwrap_or("unknown rpc error");
        return Err(message.to_string());
    }
    Ok(())
}

fn http_get_ready(rpc_addr: &str) -> Result<bool, String> {
    let response = rpc_client::http_request_bytes(
        rpc_addr,
        format!("GET /readyz HTTP/1.1\r\nHost: {rpc_addr}\r\nConnection: close\r\n\r\n").as_bytes(),
    )?;
    Ok(response.starts_with(b"HTTP/1.1 200") || response.starts_with(b"HTTP/1.0 200"))
}
