use rand_core::{OsRng, RngCore};
use rns_transport::destination::{DestinationName, SingleOutputDestination};
use rns_transport::packet::{Packet, PacketDataBuffer};
use rns_transport::transport::SendPacketOutcome;
use std::time::Duration;

const MAX_PROBE_COUNT: usize = 1_024;
const MAX_PROBE_TIMEOUT_SECS: f64 = 3_600.0;

struct ProbeSpec {
    destination: AddressHash,
    full_name: String,
    destination_name: DestinationName,
    size: usize,
    probes: usize,
    timeout: Duration,
    wait: Duration,
}

fn parse_probe_name(full_name: &str) -> Result<DestinationName, std::io::Error> {
    let mut parts = full_name.trim().split('.');
    let app_name = parts.next().unwrap_or_default();
    let aspects = parts.collect::<Vec<_>>().join(".");
    if app_name.is_empty() || aspects.is_empty() || aspects.split('.').any(str::is_empty) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "full_name must contain an application and at least one non-empty aspect",
        ));
    }
    Ok(DestinationName::new(app_name, aspects.as_str()))
}

fn probe_duration(seconds: f64, field: &str, allow_zero: bool) -> Result<Duration, std::io::Error> {
    let valid = seconds.is_finite()
        && if allow_zero {
            seconds >= 0.0
        } else {
            seconds > 0.0
        };
    if !valid {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{field} must be a finite {} number", if allow_zero { "non-negative" } else { "positive" }),
        ));
    }
    Ok(Duration::from_secs_f64(seconds))
}

async fn await_probe_identity(
    transport: &Transport,
    destination: &AddressHash,
    timeout: Duration,
) -> Result<rns_transport::identity::Identity, std::io::Error> {
    if let Some(identity) = transport.destination_identity(destination).await {
        return Ok(identity);
    }
    if !transport.await_path(destination, timeout, None).await {
        return Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("path discovery for {} timed out", destination.to_hex_string()),
        ));
    }
    transport.destination_identity(destination).await.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path discovery did not provide identity for {}", destination.to_hex_string()),
        )
    })
}

async fn run_probe_transport(
    transport: Arc<Transport>,
    probe_receipts: Arc<ProbeReceiptRegistry>,
    spec: ProbeSpec,
) -> Result<JsonValue, std::io::Error> {
    let ProbeSpec {
        destination,
        full_name,
        destination_name,
        size,
        probes,
        timeout,
        wait,
    } = spec;
    let identity = await_probe_identity(&transport, &destination, timeout).await?;
    let output = SingleOutputDestination::new(identity, destination_name);
    if output.desc.address_hash != destination {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "full_name {full_name} does not resolve to destination {}",
                destination.to_hex_string()
            ),
        ));
    }

    let mut results = Vec::with_capacity(probes);
    for probe_index in 0..probes {
        if probe_index > 0 && !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }

        let mut payload = vec![0_u8; size];
        OsRng.fill_bytes(payload.as_mut_slice());
        let packet = Packet {
            destination,
            data: PacketDataBuffer::new_from_slice(payload.as_slice()),
            ..Packet::default()
        };
        let receiver_slot = Arc::new(std::sync::Mutex::new(None));
        let receiver_slot_for_observer = receiver_slot.clone();
        let probe_receipts_for_observer = probe_receipts.clone();
        let started = std::time::Instant::now();
        let trace = transport
            .send_packet_observed_with_trace(packet, move |packet_hash| {
                let receiver = probe_receipts_for_observer.register(packet_hash.to_bytes());
                *receiver_slot_for_observer
                    .lock()
                    .expect("probe receiver slot mutex poisoned") = Some(receiver);
            })
            .await;
        let Some(packet_hash) = trace.packet_hash else {
            return Err(std::io::Error::other(format!(
                "probe packet preparation failed: {:?}",
                trace.outcome
            )));
        };
        if !matches!(trace.outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast) {
            probe_receipts.cancel(packet_hash.to_bytes());
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                format!("probe packet was not dispatched: {:?}", trace.outcome),
            ));
        }

        let receiver = receiver_slot
            .lock()
            .expect("probe receiver slot mutex poisoned")
            .take()
            .ok_or_else(|| std::io::Error::other("probe receipt registration was not observed"))?;
        let delivered = tokio::time::timeout(timeout, receiver).await.is_ok();
        if !delivered {
            probe_receipts.cancel(packet_hash.to_bytes());
        }
        results.push(json!({
            "probe": probe_index + 1,
            "status": if delivered { "delivered" } else { "timeout" },
            "rtt_ms": delivered.then(|| started.elapsed().as_secs_f64() * 1_000.0),
            "packet_hash": packet_hash.to_string(),
            "hops": transport.hops_to(&destination).await,
        }));
    }

    let replies = results
        .iter()
        .filter(|result| result.get("status").and_then(JsonValue::as_str) == Some("delivered"))
        .count();
    let packet_loss_percent = (probes.saturating_sub(replies) as f64 / probes as f64) * 100.0;
    Ok(json!({
        "full_name": full_name,
        "destination": destination.to_hex_string(),
        "size": size,
        "probes": probes,
        "sent": probes,
        "replies": replies,
        "packet_loss_percent": packet_loss_percent,
        "results": results,
    }))
}

impl DaemonPathLookupBridge {
    pub(crate) fn run_probe(
        &self,
        destination: &str,
        full_name: &str,
        size: usize,
        probes: usize,
        timeout_secs: f64,
        wait_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        let destination = parse_destination_hash_required(destination)
            .map(AddressHash::new)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let destination_name = parse_probe_name(full_name)?;
        if size > Packet::LXMF_MAX_PAYLOAD {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("size must be at most {} bytes", Packet::LXMF_MAX_PAYLOAD),
            ));
        }
        if probes == 0 || probes > MAX_PROBE_COUNT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("probes must be between 1 and {MAX_PROBE_COUNT}"),
            ));
        }
        if !timeout_secs.is_finite() || timeout_secs <= 0.0 || timeout_secs > MAX_PROBE_TIMEOUT_SECS {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("timeout must be a finite positive number no greater than {MAX_PROBE_TIMEOUT_SECS}"),
            ));
        }
        let timeout = probe_duration(timeout_secs, "timeout", false)?;
        let wait = probe_duration(wait_secs, "wait", true)?;
        let full_name = full_name.trim().to_string();
        let probe_receipts = self.probe_receipts.clone();
        self.run_transport(move |transport| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| std::io::Error::other(format!("failed to build probe runtime: {err}")))?;
            runtime.block_on(run_probe_transport(
                transport,
                probe_receipts,
                ProbeSpec {
                    destination,
                    full_name,
                    destination_name,
                    size,
                    probes,
                    timeout,
                    wait,
                },
            ))
        })
    }
}
