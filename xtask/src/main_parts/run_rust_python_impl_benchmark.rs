fn run_rust_python_impl_benchmark(name: &str, iterations: usize) -> Result<PythonBenchmark> {
    let mut samples = Vec::with_capacity(iterations);
    match name {
        "lxmf_core_message_from_wire" => {
            let (wire, _) = rust_sample_wire_payload();
            for _ in 0..iterations {
                let started = Instant::now();
                let decoded =
                    Message::from_wire(black_box(&wire)).context("decode should succeed")?;
                black_box(decoded);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "lxmf_core_message_to_wire" => {
            for _ in 0..iterations {
                let started = Instant::now();
                let mut message = Message::new();
                message.destination_hash = Some([0x44; 16]);
                message.source_hash = Some([0x55; 16]);
                message.signature = Some([0x66; 64]);
                message.timestamp = Some(1_770_000_001.0);
                message.set_title_from_string("wire-title");
                message.set_content_from_string("wire-content");
                let wire = message.to_wire(None).context("encode should succeed")?;
                black_box(wire);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "lxmf_core_large_message_from_wire" => {
            let (wire, _) = rust_sample_large_wire_payload();
            for _ in 0..iterations {
                let started = Instant::now();
                let decoded =
                    Message::from_wire(black_box(&wire)).context("decode should succeed")?;
                black_box(decoded);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "lxmf_core_large_message_to_wire" => {
            let content = "x".repeat(2048);
            for _ in 0..iterations {
                let started = Instant::now();
                let mut message = Message::new();
                message.destination_hash = Some([0xa4; 16]);
                message.source_hash = Some([0xb5; 16]);
                message.signature = Some([0xc6; 64]);
                message.timestamp = Some(1_770_000_101.0);
                message.set_title_from_string("wire-large-title");
                message.set_content_from_string(black_box(&content));
                let wire = message.to_wire(None).context("encode should succeed")?;
                black_box(wire);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_announce_create" => {
            let mut destination = rust_sample_destination();
            for _ in 0..iterations {
                let started = Instant::now();
                let packet = destination
                    .announce(OsRng, black_box(Some(b"rust-announce-app-data".as_slice())))
                    .map_err(|err| anyhow!("announce should succeed: {err:?}"))?;
                black_box(packet);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_announce_validate" => {
            let mut destination = rust_sample_destination();
            let packet = destination
                .announce(OsRng, Some(b"rust-announce-app-data".as_slice()))
                .map_err(|err| anyhow!("announce should succeed: {err:?}"))?;
            for _ in 0..iterations {
                let started = Instant::now();
                let info = DestinationAnnounce::validate(black_box(&packet))
                    .map_err(|err| anyhow!("announce validation should succeed: {err:?}"))?;
                black_box(info);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_announce_validate_batch_64" => {
            let packets = rust_announce_batch_packets()?;
            let mut signed_data = [0u8; rns_core::packet::PACKET_MDU];
            for _ in 0..iterations {
                let started = Instant::now();
                let mut validated = 0usize;
                for packet in &packets {
                    let info = DestinationAnnounce::validate_with_buffer(
                        black_box(packet),
                        black_box(&mut signed_data),
                    )
                    .map_err(|err| anyhow!("announce validation should succeed: {err:?}"))?;
                    validated += info.app_data.len();
                }
                black_box(validated);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_identity_sign" => {
            let identity = PrivateIdentity::new_from_rand(OsRng);
            let message = vec![0x5a; 2048];
            for _ in 0..iterations {
                let started = Instant::now();
                let signature = lxmf_sign(black_box(&identity), black_box(&message));
                black_box(signature);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_identity_verify" => {
            let identity = PrivateIdentity::new_from_rand(OsRng);
            let public_identity = *identity.as_identity();
            let message = vec![0x5a; 2048];
            let signature = lxmf_sign(&identity, &message);
            for _ in 0..iterations {
                let started = Instant::now();
                let valid = lxmf_verify(
                    black_box(&public_identity),
                    black_box(&message),
                    black_box(&signature),
                );
                black_box(valid);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_identity_encrypt" => {
            let recipient = PrivateIdentity::new_from_rand(OsRng);
            let public_identity = *recipient.as_identity();
            let plaintext = vec![0x42; 2048];
            let salt = public_identity.address_hash.as_slice().to_vec();
            let mut out = vec![0u8; 32 + plaintext.len() + 128];
            for _ in 0..iterations {
                let started = Instant::now();
                let ciphertext = encrypt_for_public_key_into(
                    black_box(&public_identity.public_key),
                    black_box(salt.as_slice()),
                    black_box(&plaintext),
                    black_box(out.as_mut_slice()),
                    OsRng,
                )
                .map_err(|err| anyhow!("encryption should succeed: {err:?}"))?;
                black_box(ciphertext);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_identity_decrypt" => {
            let recipient = PrivateIdentity::new_from_rand(OsRng);
            let public_identity = *recipient.as_identity();
            let plaintext = vec![0x42; 2048];
            let salt = public_identity.address_hash.as_slice().to_vec();
            let ciphertext = encrypt_for_public_key(
                &public_identity.public_key,
                salt.as_slice(),
                &plaintext,
                OsRng,
            )
            .map_err(|err| anyhow!("encryption should succeed: {err:?}"))?;
            let mut out = vec![0u8; ciphertext.len()];
            for _ in 0..iterations {
                let started = Instant::now();
                let decrypted = decrypt_with_identity_into(
                    black_box(&recipient),
                    black_box(salt.as_slice()),
                    black_box(&ciphertext),
                    black_box(out.as_mut_slice()),
                )
                .map_err(|err| anyhow!("decryption should succeed: {err:?}"))?;
                black_box(decrypted);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_transport_resource_manager_request_window_reuse" => {
            let (mut sender_link, mut manager, plain_request) =
                rust_resource_manager_request_fixture()?;
            let mut responses = Vec::new();
            for _ in 0..iterations {
                let started = Instant::now();
                manager.handle_packet_into(
                    black_box(&plain_request),
                    black_box(&mut sender_link),
                    black_box(&mut responses),
                );
                black_box(responses.len());
                samples.push(started.elapsed().as_nanos() as f64);
                responses.clear();
            }
        }
        _ => bail!("unsupported rust benchmark workload `{name}`"),
    }

    Ok(python_benchmark_from_samples(name.to_string(), iterations, samples))
}

fn python_benchmark_from_samples(
    name: String,
    iterations: usize,
    mut samples: Vec<f64>,
) -> PythonBenchmark {
    samples.sort_by(f64::total_cmp);
    let tail_samples = trimmed_tail_sample(&samples);
    let mean_ns = samples.iter().sum::<f64>() / samples.len() as f64;
    let p50_ns = percentile(&samples, 0.50);
    let p95_ns = percentile(&tail_samples, 0.95);
    let p99_ns = percentile(&tail_samples, 0.99);
    let throughput_ops_per_sec = 1_000_000_000.0 / p50_ns.max(1.0);
    PythonBenchmark { name, iterations, mean_ns, p50_ns, p95_ns, p99_ns, throughput_ops_per_sec }
}

fn rust_sample_wire_payload() -> (Vec<u8>, [u8; 16]) {
    let mut message = Message::new();
    let destination = [0x11; 16];
    let source = [0x22; 16];
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.signature = Some([0x33; 64]);
    message.timestamp = Some(1_770_000_000.0);
    message.set_title_from_string("bench-title");
    message.set_content_from_string("bench-content-payload");
    let wire = message.to_wire(None).expect("sample message must encode");
    (wire, destination)
}

fn rust_sample_large_wire_payload() -> (Vec<u8>, [u8; 16]) {
    let mut message = Message::new();
    let destination = [0x77; 16];
    let source = [0x88; 16];
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.signature = Some([0x99; 64]);
    message.timestamp = Some(1_770_000_100.0);
    message.set_title_from_string("bench-large-title");
    message.set_content_from_string(&"x".repeat(2048));
    let wire = message.to_wire(None).expect("large sample message must encode");
    (wire, destination)
}

fn rust_sample_destination() -> SingleInputDestination {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    SingleInputDestination::new(
        identity,
        DestinationName::new("example_utilities", "announcesample.fruits"),
    )
}

fn rust_announce_batch_packets() -> Result<Vec<rns_core::Packet>> {
    const ANNOUNCE_BATCH_SIZE: usize = 64;
    let mut packets = Vec::with_capacity(ANNOUNCE_BATCH_SIZE);
    for index in 0..ANNOUNCE_BATCH_SIZE {
        let mut destination = rust_sample_destination();
        let app_data = format!("rust-announce-app-data-{index}");
        let packet = destination
            .announce(OsRng, Some(app_data.as_bytes()))
            .map_err(|err| anyhow!("announce should succeed: {err:?}"))?;
        packets.push(packet);
    }
    Ok(packets)
}

fn rust_active_link_pair() -> Result<(Link, Link, Vec<u8>)> {
    let sender = PrivateIdentity::new_from_rand(OsRng);
    let receiver = PrivateIdentity::new_from_rand(OsRng);

    let _sender = to_transport_private_identity(&sender);
    let receiver = to_transport_private_identity(&receiver);

    let destination = DestinationDesc {
        identity: *receiver.as_identity(),
        address_hash: *receiver.address_hash(),
        name: TransportDestinationName::new("lxmf", "delivery"),
    };

    let (tx, _) = tokio::sync::broadcast::channel(16);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();

    let mut inbound =
        Link::new_from_request(&request, receiver.sign_key().clone(), destination, tx)
            .map_err(|err| anyhow!("input link: {err:?}"))?;
    let proof = inbound.prove();
    let proof_iface = AddressHash::new_from_rand(OsRng);
    if !matches!(outbound.handle_packet(&proof, proof_iface), LinkHandleResult::Activated) {
        bail!("link activation did not succeed");
    }

    let payload = vec![0x2a; 128];
    Ok((outbound, inbound, payload))
}

fn rust_decrypt_resource_packet(link: &Link, packet: &Packet) -> Result<Packet> {
    let mut plain_packet = *packet;
    let mut buffer = PacketDataBuffer::new();
    let plain_len = {
        let plaintext = link
            .decrypt(packet.data.as_slice(), buffer.accuire_buf_max())
            .map_err(|err| anyhow!("decrypt should succeed: {err:?}"))?;
        plaintext.len()
    };
    buffer.resize(plain_len);
    plain_packet.data = buffer;
    Ok(plain_packet)
}

fn rust_resource_manager_request_fixture() -> Result<(Link, ResourceManager, Packet)> {
    let (sender_link, mut receiver_link, _) = rust_active_link_pair()?;
    let mut sender_manager = ResourceManager::new();
    let mut receiver_manager = ResourceManager::new();
    let resource_data = vec![0x5a; PACKET_MDU * 6];

    let (_, advertisement_packet) = sender_manager
        .start_send(&sender_link, resource_data, None)
        .map_err(|err| anyhow!("resource send should succeed: {err:?}"))?;
    let plain_advertisement = rust_decrypt_resource_packet(&receiver_link, &advertisement_packet)?;

    let mut responses = Vec::new();
    receiver_manager.handle_packet_into(&plain_advertisement, &mut receiver_link, &mut responses);
    let request_packet = responses.pop().context("resource request packet")?;
    let plain_request = rust_decrypt_resource_packet(&sender_link, &request_packet)?;

    Ok((sender_link, sender_manager, plain_request))
}

fn collect_python_impl_resource_measurements(
    config: &PythonImplBenchConfig,
    per_run_reports: &[PythonImplComparisonReport],
    runs: usize,
    baseline_iterations: usize,
    min_duration_seconds: f64,
    report_root: &Path,
) -> Result<BTreeMap<String, ResourceMeasurementSet>> {
    let release_xtask = ensure_release_xtask_binary()?;
    let resources_root = report_root.join("resources");
    fs::create_dir_all(&resources_root)
        .with_context(|| format!("create {}", resources_root.display()))?;
    let time_command = detect_time_command()?;
    let mut measurements = BTreeMap::new();
    let median_rows = aggregate_report_rows_by_label(per_run_reports)?;

    for comparison in &config.comparisons {
        let rust_key = format!("rust:{}", comparison.rust_benchmark);
        let python_key = format!("python:{}", comparison.python_benchmark);
        let median_row = median_rows
            .get(&comparison.label)
            .with_context(|| format!("missing median row for `{}`", comparison.label))?;
        let rust_iterations = resource_iterations_for_duration(
            baseline_iterations,
            median_row.rust.p50_ns,
            min_duration_seconds,
        );
        let python_iterations = resource_iterations_for_duration(
            baseline_iterations,
            median_row.python.p50_ns,
            min_duration_seconds,
        );
        let rust_entries = collect_resource_measurements_for_workload(
            &time_command,
            &release_xtask,
            PythonImplImplementation::Rust,
            &comparison.rust_benchmark,
            runs,
            rust_iterations,
            &resources_root,
        )?;
        measurements.insert(
            rust_key,
            ResourceMeasurementSet {
                iterations_per_run: rust_iterations,
                measurements: rust_entries,
            },
        );

        let python_entries = collect_resource_measurements_for_workload(
            &time_command,
            &release_xtask,
            PythonImplImplementation::Python,
            &comparison.python_benchmark,
            runs,
            python_iterations,
            &resources_root,
        )?;
        measurements.insert(
            python_key,
            ResourceMeasurementSet {
                iterations_per_run: python_iterations,
                measurements: python_entries,
            },
        );
    }

    Ok(measurements)
}

#[derive(Copy, Clone)]
enum TimeCommandFlavor {
    Bsd,
    Gnu,
}

struct TimeCommand {
    program: &'static str,
    flavor: TimeCommandFlavor,
}

fn detect_time_command() -> Result<TimeCommand> {
    let program = "/usr/bin/time";
    if Command::new(program)
        .args(["-l", "true"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some()
    {
        return Ok(TimeCommand { program, flavor: TimeCommandFlavor::Bsd });
    }
    if Command::new(program)
        .args(["-v", "true"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some()
    {
        return Ok(TimeCommand { program, flavor: TimeCommandFlavor::Gnu });
    }
    bail!("unable to find a supported `/usr/bin/time` implementation")
}

fn ensure_release_xtask_binary() -> Result<PathBuf> {
    run("cargo", &["build", "-p", "xtask", "--release"])?;
    let path = Path::new("target").join("release").join(executable_name("xtask"));
    if !path.exists() {
        bail!("expected release xtask binary at {}", path.display());
    }
    Ok(path)
}

fn executable_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}
