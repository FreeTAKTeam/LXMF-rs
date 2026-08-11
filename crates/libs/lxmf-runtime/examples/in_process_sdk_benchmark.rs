use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use lxmf_runtime::{InProcessBackend, InProcessBackendConfig};
use lxmf_sdk::{MessageId, NegotiationRequest, SdkBackend};
use rand_core::OsRng;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::identity::PrivateIdentity;
use rns_transport::transport::{Transport, TransportConfig};
use serde_json::json;

const BATCH_SIZE: usize = 100;

struct Args {
    operation: String,
    iterations: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let (identity, transport) = {
        let _runtime_guard = runtime.enter();
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let transport = Arc::new(Transport::new(TransportConfig::new(
            "sdk-in-process-benchmark",
            &identity,
            true,
        )));
        (identity, transport)
    };
    let source =
        SingleInputDestination::new(identity.clone(), DestinationName::new("lxmf", "delivery"))
            .desc
            .address_hash;
    let backend = InProcessBackend::new(InProcessBackendConfig::new(
        "sdk-in-process-benchmark",
        runtime.handle().clone(),
        transport,
        identity,
        source,
    ));
    let samples = run(backend, &args.operation, args.iterations)?;
    println!("{}", serde_json::to_string(&summarize(&args, samples))?);
    Ok(())
}

fn run<B: SdkBackend>(
    backend: B,
    operation: &str,
    iterations: usize,
) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let mut samples = Vec::with_capacity(iterations);
    match operation {
        "negotiate" => {
            let request: NegotiationRequest = serde_json::from_value(json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "profile": "desktop-full",
                "bind_mode": "local_only",
                "auth_mode": "local_trusted",
                "overflow_policy": "reject",
                "block_timeout_ms": null,
                "rpc_backend": null,
                "extensions": {}
            }))?;
            backend.negotiate(request.clone())?;
            for _ in 0..iterations {
                let started = Instant::now();
                for _ in 0..BATCH_SIZE {
                    black_box(backend.negotiate(request.clone())?);
                }
                samples.push(started.elapsed().as_nanos() as f64 / BATCH_SIZE as f64);
            }
        }
        "snapshot" => {
            backend.snapshot()?;
            for _ in 0..iterations {
                let started = Instant::now();
                for _ in 0..BATCH_SIZE {
                    black_box(backend.snapshot()?);
                }
                samples.push(started.elapsed().as_nanos() as f64 / BATCH_SIZE as f64);
            }
        }
        "status" => {
            let id = MessageId("benchmark-missing-message".to_owned());
            backend.status(id.clone())?;
            for _ in 0..iterations {
                let started = Instant::now();
                for _ in 0..BATCH_SIZE {
                    black_box(backend.status(id.clone())?);
                }
                samples.push(started.elapsed().as_nanos() as f64 / BATCH_SIZE as f64);
            }
        }
        "poll_events" => {
            backend.poll_events(None, 1)?;
            for _ in 0..iterations {
                let started = Instant::now();
                for _ in 0..BATCH_SIZE {
                    black_box(backend.poll_events(None, 1)?);
                }
                samples.push(started.elapsed().as_nanos() as f64 / BATCH_SIZE as f64);
            }
        }
        "router_stats" => {
            backend.router_stats()?;
            for _ in 0..iterations {
                let started = Instant::now();
                for _ in 0..BATCH_SIZE {
                    black_box(backend.router_stats()?);
                }
                samples.push(started.elapsed().as_nanos() as f64 / BATCH_SIZE as f64);
            }
        }
        other => return Err(format!("unsupported in-process benchmark operation {other}").into()),
    }
    Ok(samples)
}

fn summarize(args: &Args, mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| {
        let index = ((samples.len() - 1) as f64 * fraction).round() as usize;
        samples[index]
    };
    json!({
        "transport": "in_process",
        "operation": args.operation,
        "iterations": args.iterations,
        "batch_size": BATCH_SIZE,
        "timed_boundary": "per-call latency normalized from fixed-size in-process batches",
        "p50_ns": percentile(0.50),
        "p95_ns": percentile(0.95),
        "p99_ns": percentile(0.99),
        "samples_ns": samples,
    })
}

fn parse_args() -> Result<Args, Box<dyn std::error::Error>> {
    let mut transport = None;
    let mut endpoint = None;
    let mut operation = None;
    let mut iterations = 100usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--transport" => transport = args.next(),
            "--endpoint" => endpoint = args.next(),
            "--operation" => operation = args.next(),
            "--iterations" => {
                iterations = args.next().ok_or("missing --iterations value")?.parse()?;
            }
            _ => return Err(format!("unknown argument {arg}").into()),
        }
    }
    if transport.as_deref() != Some("in_process") {
        return Err("--transport must be in_process".into());
    }
    if endpoint.as_deref() != Some("in-process://local") {
        return Err("--endpoint must be in-process://local".into());
    }
    if iterations == 0 {
        return Err("iterations must be positive".into());
    }
    Ok(Args { operation: operation.ok_or("missing --operation")?, iterations })
}
