use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rns_rpc::rpc::control_boundary::{
    read_control_envelope, serve_control_router, write_control_envelope, ControlCodecError,
    ControlEnvelope, ControlMessage, ControlServeStopReason,
};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;

use super::bootstrap;
use super::Args;

#[allow(dead_code)]
pub(super) struct ControlRouterStdioProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    next_sequence: u64,
}

#[allow(dead_code)]
impl ControlRouterStdioProcess {
    pub(super) fn spawn(executable: impl AsRef<Path>) -> Result<Self, ControlRouterProcessError> {
        Self::spawn_with_args(executable, std::iter::empty::<OsString>())
    }

    pub(super) fn spawn_with_args<I, S>(
        executable: impl AsRef<Path>,
        args: I,
    ) -> Result<Self, ControlRouterProcessError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let executable = executable.as_ref();
        let mut child = Command::new(executable)
            .arg("--control-router-stdio")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| ControlRouterProcessError::Spawn {
                executable: executable.to_path_buf(),
                message: err.to_string(),
            })?;
        let stdin =
            child.stdin.take().ok_or(ControlRouterProcessError::MissingPipe { name: "stdin" })?;
        let stdout =
            child.stdout.take().ok_or(ControlRouterProcessError::MissingPipe { name: "stdout" })?;
        Ok(Self { child, stdin: Some(stdin), stdout: Some(stdout), next_sequence: 1 })
    }

    pub(super) async fn request(
        &mut self,
        request: rns_rpc::RpcRequest,
    ) -> Result<rns_rpc::RpcResponse, ControlRouterProcessError> {
        self.request_inner(request).await
    }

    pub(super) async fn request_with_timeout(
        &mut self,
        request: rns_rpc::RpcRequest,
        wait: Duration,
    ) -> Result<rns_rpc::RpcResponse, ControlRouterProcessError> {
        match timeout(wait, self.request_inner(request)).await {
            Ok(result) => result,
            Err(_) => {
                let timeout_ms = wait.as_millis().min(u128::from(u64::MAX)) as u64;
                let _ = self.child.start_kill();
                drop(self.stdin.take());
                drop(self.stdout.take());
                Err(ControlRouterProcessError::RequestTimedOut { timeout_ms })
            }
        }
    }

    async fn request_inner(
        &mut self,
        request: rns_rpc::RpcRequest,
    ) -> Result<rns_rpc::RpcResponse, ControlRouterProcessError> {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let stdin =
            self.stdin.as_mut().ok_or(ControlRouterProcessError::ClosedPipe { name: "stdin" })?;
        let stdout =
            self.stdout.as_mut().ok_or(ControlRouterProcessError::ClosedPipe { name: "stdout" })?;
        write_control_envelope(stdin, &ControlEnvelope::request(sequence, request))
            .await
            .map_err(ControlRouterProcessError::Write)?;
        let envelope =
            read_control_envelope(stdout).await.map_err(ControlRouterProcessError::Read)?;
        if envelope.sequence != sequence {
            return Err(ControlRouterProcessError::SequenceMismatch {
                expected: sequence,
                actual: envelope.sequence,
            });
        }
        match envelope.message {
            ControlMessage::RpcResponse { response } => Ok(response),
            ControlMessage::RpcRequest { .. } => {
                Err(ControlRouterProcessError::UnexpectedMessage { message: "rpc_request" })
            }
            ControlMessage::Health { .. } => {
                Err(ControlRouterProcessError::UnexpectedMessage { message: "health" })
            }
            ControlMessage::Shutdown => {
                Err(ControlRouterProcessError::UnexpectedMessage { message: "shutdown" })
            }
        }
    }

    pub(super) async fn shutdown(
        mut self,
        wait: Duration,
    ) -> Result<ExitStatus, ControlRouterProcessError> {
        if let Some(stdin) = self.stdin.as_mut() {
            write_control_envelope(stdin, &ControlEnvelope::new(0, ControlMessage::Shutdown))
                .await
                .map_err(ControlRouterProcessError::Write)?;
        }
        drop(self.stdin.take());
        timeout(wait, self.child.wait())
            .await
            .map_err(|_| ControlRouterProcessError::ShutdownTimedOut)?
            .map_err(|err| ControlRouterProcessError::Wait { message: err.to_string() })
    }
}

impl Drop for ControlRouterStdioProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[allow(dead_code)]
pub(super) struct ControlRouterStdioPool {
    executable: PathBuf,
    child_args: Vec<OsString>,
    workers: Vec<Mutex<ControlRouterStdioProcess>>,
    next: AtomicUsize,
    request_timeouts: AtomicUsize,
    child_replacements: AtomicUsize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ControlRouterStdioPoolSnapshot {
    pub(super) worker_count: usize,
    pub(super) idle_workers: usize,
    pub(super) busy_workers: usize,
    pub(super) request_timeouts: usize,
    pub(super) child_replacements: usize,
}

#[allow(dead_code)]
impl ControlRouterStdioPool {
    pub(super) fn spawn(
        executable: impl AsRef<Path>,
        worker_count: usize,
    ) -> Result<Self, ControlRouterProcessError> {
        Self::spawn_with_args(executable, std::iter::empty::<OsString>(), worker_count)
    }

    pub(super) fn spawn_with_args<I, S>(
        executable: impl AsRef<Path>,
        args: I,
        worker_count: usize,
    ) -> Result<Self, ControlRouterProcessError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        if worker_count == 0 {
            return Err(ControlRouterProcessError::EmptyPool);
        }
        let executable = executable.as_ref().to_path_buf();
        let child_args = args.into_iter().map(Into::into).collect::<Vec<_>>();
        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            workers.push(Mutex::new(ControlRouterStdioProcess::spawn_with_args(
                &executable,
                child_args.iter(),
            )?));
        }
        Ok(Self {
            executable,
            child_args,
            workers,
            next: AtomicUsize::new(0),
            request_timeouts: AtomicUsize::new(0),
            child_replacements: AtomicUsize::new(0),
        })
    }

    pub(super) async fn request(
        &self,
        request: rns_rpc::RpcRequest,
        wait: Duration,
    ) -> Result<rns_rpc::RpcResponse, ControlRouterProcessError> {
        let (_index, mut worker) = self.lock_worker_for_request().await;
        match worker.request_with_timeout(request, wait).await {
            Ok(response) => Ok(response),
            Err(ControlRouterProcessError::RequestTimedOut { timeout_ms }) => {
                self.request_timeouts.fetch_add(1, Ordering::Relaxed);
                let replacement = ControlRouterStdioProcess::spawn_with_args(
                    &self.executable,
                    self.child_args.iter(),
                )?;
                let _timed_out = std::mem::replace(&mut *worker, replacement);
                self.child_replacements.fetch_add(1, Ordering::Relaxed);
                Err(ControlRouterProcessError::RequestTimedOut { timeout_ms })
            }
            Err(err) => Err(err),
        }
    }

    pub(super) fn snapshot(&self) -> ControlRouterStdioPoolSnapshot {
        let idle_workers = self.workers.iter().filter_map(|worker| worker.try_lock().ok()).count();
        let worker_count = self.workers.len();
        ControlRouterStdioPoolSnapshot {
            worker_count,
            idle_workers,
            busy_workers: worker_count.saturating_sub(idle_workers),
            request_timeouts: self.request_timeouts.load(Ordering::Relaxed),
            child_replacements: self.child_replacements.load(Ordering::Relaxed),
        }
    }

    #[cfg(test)]
    pub(super) fn child_args(&self) -> &[OsString] {
        &self.child_args
    }

    async fn lock_worker_for_request(
        &self,
    ) -> (usize, tokio::sync::MutexGuard<'_, ControlRouterStdioProcess>) {
        let start = self.next.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        for offset in 0..self.workers.len() {
            let index = (start + offset) % self.workers.len();
            if let Ok(worker) = self.workers[index].try_lock() {
                return (index, worker);
            }
        }
        (start, self.workers[start].lock().await)
    }

    pub(super) async fn shutdown(
        self,
        wait: Duration,
    ) -> Vec<Result<ExitStatus, ControlRouterProcessError>> {
        let mut results = Vec::with_capacity(self.workers.len());
        for worker in self.workers {
            results.push(worker.into_inner().shutdown(wait).await);
        }
        results
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(super) enum ControlRouterProcessError {
    InvalidConfig { message: String },
    EmptyPool,
    Spawn { executable: PathBuf, message: String },
    MissingPipe { name: &'static str },
    ClosedPipe { name: &'static str },
    Write(ControlCodecError),
    Read(ControlCodecError),
    UnexpectedMessage { message: &'static str },
    SequenceMismatch { expected: u64, actual: u64 },
    RequestTimedOut { timeout_ms: u64 },
    Wait { message: String },
    ShutdownTimedOut,
}

pub(super) fn validate_control_router_process_options(
    worker_count: usize,
    timeout_ms: u64,
) -> Result<(), ControlRouterProcessError> {
    if worker_count == 0 {
        return Ok(());
    }
    if timeout_ms == 0 {
        return Err(ControlRouterProcessError::InvalidConfig {
            message:
                "control router process timeout must be greater than zero when workers are enabled"
                    .to_string(),
        });
    }
    Ok(())
}

pub(super) async fn run_control_router_stdio(args: Args) {
    let context = bootstrap::bootstrap(args).await;
    let daemon = context.daemon.clone();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    match serve_control_router(&mut stdin, &mut stdout, |request| {
        let daemon = daemon.clone();
        async move {
            let id = request.id;
            daemon.handle_rpc(request).unwrap_or_else(|err| rns_rpc::RpcResponse {
                id,
                result: None,
                error: Some(rns_rpc::RpcError {
                    code: "internal_error".to_string(),
                    message: err.to_string(),
                    machine_code: None,
                    category: None,
                    retryable: None,
                    is_user_actionable: None,
                    details: None,
                    cause_code: None,
                    extensions: None,
                }),
            })
        }
    })
    .await
    {
        Ok(summary) => match summary.stop_reason {
            ControlServeStopReason::Shutdown | ControlServeStopReason::Eof => {}
        },
        Err(err) => eprintln!("[control-router-stdio] stopped err={err:?}"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    use rns_rpc::{RpcRequest, RpcResponse};
    use serde_json::json;

    use super::*;

    #[test]
    fn control_router_process_options_allow_disabled_pool_with_zero_timeout() {
        validate_control_router_process_options(0, 0)
            .expect("disabled control router pool should ignore timeout");
    }

    #[test]
    fn control_router_process_options_reject_enabled_pool_with_zero_timeout() {
        let err =
            validate_control_router_process_options(1, 0).expect_err("zero timeout should fail");
        assert!(matches!(err, ControlRouterProcessError::InvalidConfig { .. }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn control_router_stdio_process_round_trips_rpc_response() {
        let temp = tempfile::tempdir().expect("temp dir");
        let script = temp.path().join("control-router-mock.py");
        let response_frame = ControlEnvelope::response(
            1,
            RpcResponse { id: 42, result: Some(json!({"ok": true})), error: None },
        )
        .encode_frame()
        .expect("response frame");
        fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import struct
import sys

header = sys.stdin.buffer.read(4)
if len(header) != 4:
    sys.exit(2)
length = struct.unpack(">I", header)[0]
payload = sys.stdin.buffer.read(length)
if len(payload) != length:
    sys.exit(3)
sys.stdout.buffer.write(bytes.fromhex({response_hex:?}))
sys.stdout.buffer.flush()

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
"#,
                response_hex = hex::encode(response_frame),
            ),
        )
        .expect("write mock control router");
        let mut permissions = fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("script permissions");

        let mut process = ControlRouterStdioProcess::spawn(&script).expect("spawn process");
        let response = process
            .request(RpcRequest { id: 42, method: "daemon_status_ex".to_string(), params: None })
            .await
            .expect("control response");
        assert_eq!(response.id, 42);
        assert_eq!(response.result, Some(json!({"ok": true})));
        let status = process.shutdown(Duration::from_secs(2)).await.expect("shutdown process");
        assert!(status.success(), "mock control router exited with {status}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn control_router_stdio_process_times_out_stalled_child() {
        let temp = tempfile::tempdir().expect("temp dir");
        let script = temp.path().join("control-router-stalled.py");
        fs::write(
            &script,
            r#"#!/usr/bin/env python3
import struct
import time
import sys

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
time.sleep(60)
"#,
        )
        .expect("write stalled control router");
        let mut permissions = fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("script permissions");

        let mut process = ControlRouterStdioProcess::spawn(&script).expect("spawn process");
        let result = process
            .request_with_timeout(
                RpcRequest { id: 43, method: "daemon_status_ex".to_string(), params: None },
                Duration::from_millis(25),
            )
            .await;
        assert!(matches!(
            result,
            Err(ControlRouterProcessError::RequestTimedOut { timeout_ms: 25 })
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn control_router_stdio_pool_serves_idle_child_while_peer_is_stalled() {
        let temp = tempfile::tempdir().expect("temp dir");
        let script = temp.path().join("control-router-pool.py");
        let response_frame = ControlEnvelope::response(
            1,
            RpcResponse { id: 2, result: Some(json!({"worker": "idle"})), error: None },
        )
        .encode_frame()
        .expect("response frame");
        fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import sys
import struct

header = sys.stdin.buffer.read(4)
if len(header) != 4:
    sys.exit(2)
length = struct.unpack(">I", header)[0]
payload = sys.stdin.buffer.read(length)
if len(payload) != length:
    sys.exit(3)
sys.stdout.buffer.write(bytes.fromhex({response_hex:?}))
sys.stdout.buffer.flush()
header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
"#,
                response_hex = hex::encode(response_frame),
            ),
        )
        .expect("write pool control router");
        let mut permissions = fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = Arc::new(ControlRouterStdioPool::spawn(&script, 2).expect("spawn pool"));
        let busy_slot = pool.workers[0].lock().await;
        let snapshot = pool.snapshot();
        assert_eq!(snapshot.worker_count, 2);
        assert_eq!(snapshot.busy_workers, 1);
        assert_eq!(snapshot.idle_workers, 1);

        let response = timeout(
            Duration::from_secs(2),
            pool.request(
                RpcRequest { id: 2, method: "daemon_status_ex".to_string(), params: None },
                Duration::from_secs(2),
            ),
        )
        .await
        .expect("idle child request timeout")
        .expect("idle child response");
        assert_eq!(response.id, 2);
        assert_eq!(response.result, Some(json!({"worker": "idle"})));
        drop(busy_slot);
        let pool = match Arc::try_unwrap(pool) {
            Ok(pool) => pool,
            Err(_) => panic!("pool should have no remaining references"),
        };
        let _ = pool.shutdown(Duration::from_millis(100)).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn control_router_stdio_pool_replaces_timed_out_child() {
        let temp = tempfile::tempdir().expect("temp dir");
        let script = temp.path().join("control-router-replacement.py");
        let stall_token = temp.path().join("stall-token");
        let response_frame = ControlEnvelope::response(
            1,
            RpcResponse { id: 11, result: Some(json!({"replacement": true})), error: None },
        )
        .encode_frame()
        .expect("response frame");
        fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import struct
import sys
import time

stall_token = {stall_token_path:?}
try:
    token = open(stall_token, "x")
    token.write("stalled")
    token.close()
    should_stall = True
except FileExistsError:
    should_stall = False
header = sys.stdin.buffer.read(4)
if len(header) != 4:
    sys.exit(2)
length = struct.unpack(">I", header)[0]
payload = sys.stdin.buffer.read(length)
if len(payload) != length:
    sys.exit(3)
if should_stall:
    time.sleep(60)
else:
    sys.stdout.buffer.write(bytes.fromhex({response_hex:?}))
    sys.stdout.buffer.flush()
    header = sys.stdin.buffer.read(4)
    if len(header) == 4:
        length = struct.unpack(">I", header)[0]
        sys.stdin.buffer.read(length)
"#,
                stall_token_path = stall_token.to_string_lossy(),
                response_hex = hex::encode(response_frame),
            ),
        )
        .expect("write replacement control router");
        let mut permissions = fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = ControlRouterStdioPool::spawn(&script, 1).expect("spawn pool");
        let first = pool
            .request(
                RpcRequest { id: 10, method: "daemon_status_ex".to_string(), params: None },
                Duration::from_millis(500),
            )
            .await;
        assert!(matches!(
            first,
            Err(ControlRouterProcessError::RequestTimedOut { timeout_ms: 500 })
        ));
        assert_eq!(pool.snapshot().request_timeouts, 1);
        assert_eq!(pool.snapshot().child_replacements, 1);
        if !stall_token.exists() {
            fs::write(&stall_token, "stalled").expect("force replacement path marker");
        }

        let second = pool
            .request(
                RpcRequest { id: 11, method: "daemon_status_ex".to_string(), params: None },
                Duration::from_secs(5),
            )
            .await
            .expect("replacement response");
        assert_eq!(second.id, 11);
        assert_eq!(second.result, Some(json!({"replacement": true})));
        let _ = pool.shutdown(Duration::from_millis(100)).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn control_router_status_publisher_reports_pool_health() {
        let temp = tempfile::tempdir().expect("temp dir");
        let script = temp.path().join("control-router-status.py");
        fs::write(
            &script,
            r#"#!/usr/bin/env python3
import struct
import sys

while True:
    header = sys.stdin.buffer.read(4)
    if len(header) != 4:
        sys.exit(0)
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
"#,
        )
        .expect("write status control router");
        let mut permissions = fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = Arc::new(ControlRouterStdioPool::spawn(&script, 1).expect("spawn pool"));
        let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
        let runtime = crate::bootstrap::ControlRouterProcessRuntimeStatus {
            enabled: true,
            worker_count: 1,
            timeout_ms: 750,
        };
        crate::bootstrap::refresh_control_router_process_status(
            daemon.as_ref(),
            &runtime,
            Some(pool.as_ref()),
        );
        let publisher =
            crate::bootstrap::spawn_control_router_process_status_publisher_with_interval(
                daemon.clone(),
                runtime,
                Some(pool.clone()),
                Duration::from_millis(10),
            )
            .expect("control router status publisher should start with pool");
        let status = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 5101,
                method: "daemon_status_ex".to_string(),
                params: None,
            })
            .expect("control router status")
            .result
            .expect("control router status result");
        assert_eq!(status["control_router_processes"]["enabled"], serde_json::json!(true));
        assert_eq!(status["control_router_processes"]["worker_count"], serde_json::json!(1));
        assert_eq!(status["control_router_processes"]["timeout_ms"], serde_json::json!(750));
        assert_eq!(status["control_router_processes"]["idle_workers"], serde_json::json!(1));
        assert_eq!(status["control_router_processes"]["busy_workers"], serde_json::json!(0));
        publisher.abort();
    }
}
