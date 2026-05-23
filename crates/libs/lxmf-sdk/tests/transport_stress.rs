#![cfg(all(feature = "rpc-backend", feature = "zmq-pipeline-backend"))]
#![allow(clippy::result_large_err)]

use lxmf_sdk::{
    ConfigPatch, EventCursor, LxmfSdk, RpcBackendClient, SdkBackend, SdkConfig, SdkError,
    StartRequest, ZmqEndpointRole, ZmqPipelineBackendClient, ZmqPipelineBackendConfig,
    ZmqPipelineTokenAuth,
};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct StressStats {
    label: &'static str,
    iterations: usize,
    elapsed: Duration,
}

impl StressStats {
    fn ops_per_second(&self) -> f64 {
        self.iterations as f64 / self.elapsed.as_secs_f64().max(f64::EPSILON)
    }

    fn avg_latency_us(&self) -> f64 {
        self.elapsed.as_micros() as f64 / self.iterations.max(1) as f64
    }
}

#[test]
fn formats_terse_transport_stress_report_line() {
    let http = StressStats { label: "http", iterations: 100, elapsed: Duration::from_millis(50) };
    let zmq = StressStats { label: "zmq", iterations: 100, elapsed: Duration::from_millis(25) };

    let line = format_comparison("snapshot", &http, &zmq);

    assert_eq!(
        line,
        "transport_stress op=snapshot iterations=100 http_ms=50 http_avg_us=500.00 http_ops=2000.00 zmq_ms=25 zmq_avg_us=250.00 zmq_ops=4000.00 zmq_http_ratio=2.000"
    );
}

#[test]
#[ignore = "requires live HTTP and ZeroMQ daemon endpoints"]
fn compare_http_and_zmq_sdk_transport_stress() -> Result<(), SdkError> {
    let iterations = stress_iterations();
    let http = RpcBackendClient::new(http_endpoint());
    let zmq = ZmqPipelineBackendClient::new(zmq_config())?;

    warm_up_backend(&http)?;
    warm_up_backend(&zmq)?;
    configure_stress_rate_limits(&http, iterations)?;

    let http_stats = stress_snapshot("http", &http, iterations)?;
    let zmq_stats = stress_snapshot("zmq", &zmq, iterations)?;

    println!("{}", format_comparison("snapshot", &http_stats, &zmq_stats));

    let http_stats = stress_poll_events("http", &http, iterations)?;
    let zmq_stats = stress_poll_events("zmq", &zmq, iterations)?;

    println!("{}", format_comparison("poll_events", &http_stats, &zmq_stats));
    Ok(())
}

fn configure_stress_rate_limits<B>(backend: &B, iterations: usize) -> Result<(), SdkError>
where
    B: SdkBackend,
{
    let snapshot = backend.snapshot()?;
    let limit = u64::try_from(iterations).unwrap_or(u64::MAX / 4).saturating_mul(4).max(1_000);
    let patch = ConfigPatch::new().with_extension(
        "rate_limits",
        json!({
            "per_ip_per_minute": limit,
            "per_principal_per_minute": limit,
        }),
    );
    let _ = backend.configure(snapshot.config_revision, patch)?;
    Ok(())
}

fn warm_up_backend<B>(backend: &B) -> Result<(), SdkError>
where
    B: SdkBackend,
{
    let client = lxmf_sdk::Client::new(backend_ref(backend));
    let _ = client.start(StartRequest::new(SdkConfig::desktop_local_default()))?;
    let _ = backend.snapshot()?;
    Ok(())
}

fn stress_snapshot<B>(
    label: &'static str,
    backend: &B,
    iterations: usize,
) -> Result<StressStats, SdkError>
where
    B: SdkBackend,
{
    let started = Instant::now();
    for _ in 0..iterations {
        let _ = backend.snapshot()?;
    }
    Ok(StressStats { label, iterations, elapsed: started.elapsed() })
}

fn stress_poll_events<B>(
    label: &'static str,
    backend: &B,
    iterations: usize,
) -> Result<StressStats, SdkError>
where
    B: SdkBackend,
{
    let mut cursor: Option<EventCursor> = None;
    let started = Instant::now();
    for _ in 0..iterations {
        let batch = backend.poll_events(cursor.clone(), 64)?;
        cursor = Some(batch.next_cursor);
    }
    Ok(StressStats { label, iterations, elapsed: started.elapsed() })
}

fn format_comparison(op: &str, http: &StressStats, zmq: &StressStats) -> String {
    debug_assert_eq!(http.label, "http");
    debug_assert_eq!(zmq.label, "zmq");
    format!(
        "transport_stress op={} iterations={} http_ms={} http_avg_us={:.2} http_ops={:.2} zmq_ms={} zmq_avg_us={:.2} zmq_ops={:.2} zmq_http_ratio={:.3}",
        op,
        http.iterations,
        http.elapsed.as_millis(),
        http.avg_latency_us(),
        http.ops_per_second(),
        zmq.elapsed.as_millis(),
        zmq.avg_latency_us(),
        zmq.ops_per_second(),
        zmq.ops_per_second() / http.ops_per_second().max(f64::EPSILON)
    )
}

fn http_endpoint() -> String {
    std::env::var("LXMF_STRESS_HTTP_RPC")
        .or_else(|_| std::env::var("LXMF_RPC"))
        .unwrap_or_else(|_| "127.0.0.1:4242".to_owned())
}

fn zmq_config() -> ZmqPipelineBackendConfig {
    let command_endpoint = std::env::var("LXMF_STRESS_ZMQ_COMMAND")
        .unwrap_or_else(|_| "tcp://127.0.0.1:9100".to_owned());
    let response_endpoint = std::env::var("LXMF_STRESS_ZMQ_RESPONSE")
        .unwrap_or_else(|_| "tcp://127.0.0.1:9101".to_owned());
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.command_role =
        endpoint_role_from_env("LXMF_STRESS_ZMQ_COMMAND_ROLE", config.command_role);
    config.response_role =
        endpoint_role_from_env("LXMF_STRESS_ZMQ_RESPONSE_ROLE", config.response_role);
    config.request_timeout = Duration::from_millis(
        std::env::var("LXMF_STRESS_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5_000),
    );
    config.token_auth = token_auth_from_env();
    config
}

fn endpoint_role_from_env(name: &str, default_role: ZmqEndpointRole) -> ZmqEndpointRole {
    match std::env::var(name).ok().as_deref() {
        Some("bind") | Some("BIND") => ZmqEndpointRole::Bind,
        Some("connect") | Some("CONNECT") => ZmqEndpointRole::Connect,
        _ => default_role,
    }
}

fn token_auth_from_env() -> Option<ZmqPipelineTokenAuth> {
    let shared_secret = std::env::var("LXMF_STRESS_ZMQ_TOKEN_SECRET").ok()?;
    Some(ZmqPipelineTokenAuth {
        issuer: std::env::var("LXMF_STRESS_ZMQ_TOKEN_ISSUER")
            .unwrap_or_else(|_| "lxmf-sdk-stress".to_owned()),
        audience: std::env::var("LXMF_STRESS_ZMQ_TOKEN_AUDIENCE")
            .unwrap_or_else(|_| "reticulumd-zmq".to_owned()),
        shared_secret,
        ttl_secs: std::env::var("LXMF_STRESS_ZMQ_TOKEN_TTL_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(60),
    })
}

fn stress_iterations() -> usize {
    std::env::var("LXMF_STRESS_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1_000)
}

fn backend_ref<'a, B: SdkBackend>(backend: &'a B) -> BackendRef<'a, B> {
    BackendRef(backend)
}

struct BackendRef<'a, B>(&'a B);

impl<B> SdkBackend for BackendRef<'_, B>
where
    B: SdkBackend,
{
    fn negotiate(
        &self,
        req: lxmf_sdk::NegotiationRequest,
    ) -> Result<lxmf_sdk::NegotiationResponse, SdkError> {
        self.0.negotiate(req)
    }

    fn send(&self, req: lxmf_sdk::SendRequest) -> Result<lxmf_sdk::MessageId, SdkError> {
        self.0.send(req)
    }

    fn cancel(&self, id: lxmf_sdk::MessageId) -> Result<lxmf_sdk::CancelResult, SdkError> {
        self.0.cancel(id)
    }

    fn status(
        &self,
        id: lxmf_sdk::MessageId,
    ) -> Result<Option<lxmf_sdk::DeliverySnapshot>, SdkError> {
        self.0.status(id)
    }

    fn configure(
        &self,
        expected_revision: u64,
        patch: lxmf_sdk::ConfigPatch,
    ) -> Result<lxmf_sdk::Ack, SdkError> {
        self.0.configure(expected_revision, patch)
    }

    fn poll_events(
        &self,
        cursor: Option<EventCursor>,
        max: usize,
    ) -> Result<lxmf_sdk::EventBatch, SdkError> {
        self.0.poll_events(cursor, max)
    }

    fn snapshot(&self) -> Result<lxmf_sdk::RuntimeSnapshot, SdkError> {
        self.0.snapshot()
    }

    fn shutdown(&self, mode: lxmf_sdk::ShutdownMode) -> Result<lxmf_sdk::Ack, SdkError> {
        self.0.shutdown(mode)
    }
}
