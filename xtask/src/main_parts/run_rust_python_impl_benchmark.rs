fn run_rust_python_impl_benchmark(name: &str, iterations: usize) -> Result<PythonBenchmark> {
    let mut samples = Vec::with_capacity(iterations);
    match name {
        "lxmf_core_message_from_wire"
        | "lxmf_core_large_message_from_wire"
        | "lxmf_core_resource_message_from_wire" => {
            // Matches LXMessage.unpack_from_bytes, which validates the signature.
            let signer = PrivateIdentity::new_from_rand(OsRng);
            let identity = *signer.as_identity();
            let (title, content) = rust_lxmf_bench_text(name);
            let wire = rust_unsigned_message(title, &content)
                .to_wire(Some(&signer))
                .context("encode sample message")?;
            for _ in 0..iterations {
                let started = Instant::now();
                let valid = WireMessage::unpack(black_box(&wire))
                    .context("decode should succeed")?
                    .verify(&identity)
                    .context("verify should succeed")?;
                ensure!(valid, "sample message signature must verify");
                black_box(valid);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "lxmf_core_message_to_wire"
        | "lxmf_core_large_message_to_wire"
        | "lxmf_core_resource_message_to_wire" => {
            // Matches LXMessage.pack(), which signs.
            let signer = PrivateIdentity::new_from_rand(OsRng);
            let (title, content) = rust_lxmf_bench_text(name);
            for _ in 0..iterations {
                let started = Instant::now();
                let wire = rust_unsigned_message(title, black_box(&content))
                    .to_wire(Some(&signer))
                    .context("encode should succeed")?;
                black_box(wire);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_packet_pack" => {
            let packet = Packet {
                destination: AddressHash::new([0x42; 16]),
                data: PacketDataBuffer::new_from_slice(&[0x51; 128]),
                ..Default::default()
            };
            for _ in 0..iterations {
                let started = Instant::now();
                let packed = packet
                    .to_bytes()
                    .map_err(|error| anyhow!("packet pack should succeed: {error:?}"))?;
                black_box(packed);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_packet_unpack" => {
            let packet = Packet {
                destination: AddressHash::new([0x42; 16]),
                data: PacketDataBuffer::new_from_slice(&[0x51; 128]),
                ..Default::default()
            };
            let packed = packet
                .to_bytes()
                .map_err(|error| anyhow!("packet pack should succeed: {error:?}"))?;
            for _ in 0..iterations {
                let started = Instant::now();
                let unpacked = Packet::from_bytes(black_box(&packed))
                    .map_err(|error| anyhow!("packet unpack should succeed: {error:?}"))?;
                black_box(unpacked);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_transport_resource_segment_16k" => {
            let payload = shared_fixture_payload("resource_content_length");
            for _ in 0..iterations {
                let started = Instant::now();
                let packets = Packet::fragment_for_lxmf(black_box(&payload))
                    .map_err(|error| anyhow!("resource segmentation should succeed: {error:?}"))?;
                black_box(packets);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_transport_resource_reassemble_16k" => {
            let packets = Packet::fragment_for_lxmf(&shared_fixture_payload("resource_content_length"))
                .map_err(|error| anyhow!("resource segmentation should succeed: {error:?}"))?;
            for _ in 0..iterations {
                let started = Instant::now();
                let mut payload = Vec::with_capacity(16_384);
                for packet in black_box(&packets) {
                    payload.extend_from_slice(packet.data.as_slice());
                }
                black_box(payload);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_announce_create" => {
            let mut destination = rust_sample_destination();
            let app_data = shared_fixture_announce_data();
            for _ in 0..iterations {
                let started = Instant::now();
                let packet = destination
                    .announce(OsRng, black_box(Some(app_data.as_slice())))
                    .map_err(|err| anyhow!("announce should succeed: {err:?}"))?;
                black_box(packet);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_announce_validate" => {
            let mut destination = rust_sample_destination();
            let app_data = shared_fixture_announce_data();
            let packet = destination
                .announce(OsRng, Some(app_data.as_slice()))
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
