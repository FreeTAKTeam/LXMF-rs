//! Issue #663 resource-soak client: an RCH-shaped ZeroMQ event poller.
//!
//! Negotiates SDK contract v2 over the daemon's ZeroMQ endpoint, then polls
//! events on a fixed interval (RCH uses 100 ms) for the requested duration.
//! Prints one JSON line per report interval with poll, error and latency
//! counters so the Python sampler can align them with process and cgroup data.

use clap::Parser;
use lxmf_sdk::{
    EventCursor, NegotiationRequest, SdkBackend, SdkError, ZmqPipelineBackendClient,
    ZmqPipelineBackendConfig,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(name = "resource_soak_663_client")]
struct Args {
    /// Daemon ZeroMQ endpoint, for example tcp://127.0.0.1:9100. Alone it is the
    /// single ROUTER/DEALER endpoint; with --response-endpoint it is the legacy
    /// PUSH/PULL command endpoint RCH uses in production.
    #[arg(long)]
    endpoint: String,
    /// Local PULL endpoint to bind for legacy dual-endpoint replies.
    #[arg(long)]
    response_endpoint: Option<String>,
    /// Delay between successive event polls.
    #[arg(long, default_value_t = 100)]
    poll_interval_ms: u64,
    /// Maximum events requested per poll.
    #[arg(long, default_value_t = 32)]
    poll_max: usize,
    /// Interval between JSON report lines.
    #[arg(long, default_value_t = 5)]
    report_secs: u64,
    /// Total run time; 0 runs until interrupted.
    #[arg(long, default_value_t = 600)]
    duration_secs: u64,
}

#[derive(Default)]
struct Window {
    polls: u64,
    errors: u64,
    events: u64,
    errors_by_code: BTreeMap<String, u64>,
    latencies_us: Vec<u64>,
}

impl Window {
    fn record(&mut self, elapsed: Duration, outcome: Result<usize, &SdkError>) {
        self.polls += 1;
        self.latencies_us.push(elapsed.as_micros().min(u128::from(u64::MAX)) as u64);
        match outcome {
            Ok(events) => self.events += events as u64,
            Err(error) => {
                self.errors += 1;
                *self.errors_by_code.entry(error.machine_code.clone()).or_insert(0) += 1;
            }
        }
    }

    fn report(&mut self, totals: &Totals, started: Instant) -> serde_json::Value {
        self.latencies_us.sort_unstable();
        let percentile = |fraction: f64| -> u64 {
            if self.latencies_us.is_empty() {
                return 0;
            }
            let index = ((self.latencies_us.len() - 1) as f64 * fraction).round() as usize;
            self.latencies_us[index.min(self.latencies_us.len() - 1)]
        };
        json!({
            "schema": "lxmf-rs-resource-soak-663-client-v1",
            "unix_time": unix_now(),
            "elapsed_secs": started.elapsed().as_secs_f64(),
            "window": {
                "polls": self.polls,
                "errors": self.errors,
                "events": self.events,
                "errors_by_code": self.errors_by_code,
                "latency_us": {
                    "p50": percentile(0.50),
                    "p95": percentile(0.95),
                    "max": self.latencies_us.last().copied().unwrap_or(0),
                },
            },
            "total": {
                "polls": totals.polls,
                "errors": totals.errors,
                "events": totals.events,
                "errors_by_code": totals.errors_by_code,
            },
        })
    }
}

#[derive(Default)]
struct Totals {
    polls: u64,
    errors: u64,
    events: u64,
    errors_by_code: BTreeMap<String, u64>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let config = match &args.response_endpoint {
        Some(response) => {
            ZmqPipelineBackendConfig::local_tcp(args.endpoint.clone(), response.clone())
        }
        None => ZmqPipelineBackendConfig::local(args.endpoint.clone()),
    };
    let backend = ZmqPipelineBackendClient::new(config)?;
    let request: NegotiationRequest = serde_json::from_value(json!({
        "supported_contract_versions": [2],
        "requested_capabilities": ["sdk.capability.async_events", "sdk.capability.cursor_replay"],
        "profile": "desktop-local-runtime",
        "bind_mode": "local_only",
        "auth_mode": "local_trusted",
        "overflow_policy": "reject",
        "block_timeout_ms": null,
        "rpc_backend": null,
        "extensions": {}
    }))?;
    let negotiated = backend.negotiate(request)?;
    println!(
        "{}",
        json!({
            "schema": "lxmf-rs-resource-soak-663-client-v1",
            "unix_time": unix_now(),
            "negotiated": serde_json::to_value(&negotiated)?,
        })
    );

    let started = Instant::now();
    let deadline =
        (args.duration_secs > 0).then(|| started + Duration::from_secs(args.duration_secs));
    let poll_interval = Duration::from_millis(args.poll_interval_ms);
    let report_interval = Duration::from_secs(args.report_secs.max(1));
    let mut next_report = started + report_interval;
    let mut cursor: Option<EventCursor> = None;
    let mut window = Window::default();
    let mut totals = Totals::default();

    loop {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        let poll_started = Instant::now();
        let result = backend.poll_events(cursor.clone(), args.poll_max);
        let elapsed = poll_started.elapsed();
        totals.polls += 1;
        match &result {
            Ok(batch) => {
                totals.events += batch.events.len() as u64;
                cursor = Some(batch.next_cursor.clone());
                window.record(elapsed, Ok(batch.events.len()));
            }
            Err(error) => {
                totals.errors += 1;
                *totals.errors_by_code.entry(error.machine_code.clone()).or_insert(0) += 1;
                window.record(elapsed, Err(error));
            }
        }
        if Instant::now() >= next_report {
            println!("{}", window.report(&totals, started));
            window = Window::default();
            next_report += report_interval;
        }
        std::thread::sleep(poll_interval.saturating_sub(elapsed));
    }
    println!("{}", window.report(&totals, started));
    Ok(())
}

fn unix_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}
