use std::process::Stdio;
use std::time::{Duration, Instant};

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand_core::OsRng;
use rns_rpc::rpc::codec;
use rns_rpc::rpc::control_boundary::{
    read_control_envelope, write_control_envelope, ControlEnvelope, ControlMessage,
};
use rns_rpc::RpcRequest;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::identity::PrivateIdentity;
use rns_transport::packet::{DestinationType, Header, Packet, PacketDataBuffer, PacketType};
use rns_transport::ratchets::encrypt_for_public_key_bytes;
use rns_transport::transport::worker_boundary::{
    read_worker_frame, write_worker_frame, OutboundEncryptBatchItem, WorkerJob, WorkerJobKind,
    WorkerRequest, WorkerResponse, WorkerResultKind, MAX_WORKER_REQUEST_BYTES,
    MAX_WORKER_RESPONSE_BYTES,
};
use serde_json::json;
use sha2::Digest;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::runtime::Runtime;
use tokio::{io::AsyncReadExt, io::AsyncWriteExt, net::TcpStream};

fn unencrypted_resource_complete_job(id: u64, payload: &[u8]) -> WorkerJob {
    let random_hash = [0x5a; rns_transport::resource::RANDOM_HASH_SIZE];
    let mut hasher = sha2::Sha256::new();
    hasher.update(payload);
    hasher.update(random_hash);
    let digest = hasher.finalize();
    let mut resource_hash = [0u8; rns_transport::hash::HASH_SIZE];
    resource_hash.copy_from_slice(&digest[..rns_transport::hash::HASH_SIZE]);
    let mut stream = random_hash.to_vec();
    stream.extend_from_slice(payload);

    WorkerJob {
        id,
        kind: WorkerJobKind::ResourceComplete {
            link_id: [0x11; rns_transport::hash::ADDRESS_HASH_SIZE],
            link_context: None,
            resource_hash,
            random_hash,
            encrypted: false,
            compressed: false,
            has_metadata: false,
            data_size: payload.len() as u64,
            request_id: None,
            is_request: false,
            is_response: false,
            stream: serde_bytes::ByteBuf::from(stream),
        },
    }
}

fn outbound_encrypt_job(id: u64, payload: &[u8]) -> WorkerJob {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Single,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        destination: destination.desc.address_hash,
        data: PacketDataBuffer::new_from_slice(payload),
        ..Default::default()
    };
    let mut salt = [0u8; rns_transport::hash::ADDRESS_HASH_SIZE];
    salt.copy_from_slice(destination.desc.identity.address_hash.as_slice());
    WorkerJob {
        id,
        kind: WorkerJobKind::OutboundEncrypt {
            packet_wire: packet.to_bytes().expect("packet wire"),
            public_key: *destination.desc.identity.public_key.as_bytes(),
            salt,
        },
    }
}

fn outbound_encrypt_batch_job(id: u64, payload: &[u8], count: usize) -> WorkerJob {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let mut salt = [0u8; rns_transport::hash::ADDRESS_HASH_SIZE];
    salt.copy_from_slice(destination.desc.identity.address_hash.as_slice());
    let items = (0..count)
        .map(|_| {
            let packet = Packet {
                header: Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: destination.desc.address_hash,
                data: PacketDataBuffer::new_from_slice(payload),
                ..Default::default()
            };
            OutboundEncryptBatchItem {
                packet_wire: packet.to_bytes().expect("packet wire"),
                public_key: *destination.desc.identity.public_key.as_bytes(),
                salt,
            }
        })
        .collect();
    WorkerJob { id, kind: WorkerJobKind::OutboundEncryptBatch { items } }
}

fn complete_outbound_encrypt_locally(kind: &WorkerJobKind) -> Vec<u8> {
    let WorkerJobKind::OutboundEncrypt { packet_wire, public_key, salt } = kind else {
        panic!("expected outbound encrypt job");
    };
    let mut packet = Packet::from_bytes(packet_wire).expect("decode outbound packet");
    let ciphertext = encrypt_for_public_key_bytes(public_key, salt, packet.data.as_slice(), OsRng)
        .expect("outbound encrypt");
    let mut buffer = PacketDataBuffer::new();
    buffer.write(&ciphertext).expect("encrypted packet fits");
    packet.data = buffer;
    packet.to_bytes().expect("encode encrypted packet")
}

fn complete_outbound_encrypt_batch_locally(kind: &WorkerJobKind) -> usize {
    let WorkerJobKind::OutboundEncryptBatch { items } = kind else {
        panic!("expected outbound encrypt batch job");
    };
    items
        .iter()
        .map(|item| {
            let kind = WorkerJobKind::OutboundEncrypt {
                packet_wire: item.packet_wire.clone(),
                public_key: item.public_key,
                salt: item.salt,
            };
            complete_outbound_encrypt_locally(&kind).len()
        })
        .sum()
}

fn worker_executable() -> String {
    std::env::var("CARGO_BIN_EXE_reticulumd").unwrap_or_else(|_| {
        let mut exe = std::env::current_exe().expect("current benchmark executable path");
        while exe.file_name().is_some_and(|name| name != "target") {
            exe.pop();
        }
        exe.push("release");
        exe.push("reticulumd");
        exe.to_string_lossy().into_owned()
    })
}

struct WorkerChild {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

struct ControlRouterChild {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    _temp_dir: tempfile::TempDir,
    next_sequence: u64,
}

struct RoutedHttpDaemon {
    child: Child,
    addr: std::net::SocketAddr,
    _temp_dir: tempfile::TempDir,
}

impl RoutedHttpDaemon {
    async fn spawn() -> Self {
        let temp_dir = tempfile::tempdir().expect("routed control router temp dir");
        let db_path = temp_dir.path().join("reticulum.db");
        let addr = unused_loopback_addr();
        let mut child = Command::new(worker_executable())
            .arg("--db")
            .arg(db_path)
            .arg("--rpc")
            .arg(addr.to_string())
            .arg("--no-rpc-unix")
            .arg("--control-router-process-count")
            .arg("1")
            .arg("--control-router-process-timeout-ms")
            .arg("5000")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn routed reticulumd process");
        wait_for_tcp_rpc(addr, &mut child).await;
        let daemon = Self { child, addr, _temp_dir: temp_dir };
        daemon.disable_rate_limits().await;
        daemon
    }

    async fn request(&self, request: &[u8]) -> rns_rpc::RpcResponse {
        let mut stream = TcpStream::connect(self.addr).await.expect("connect routed rpc socket");
        stream.write_all(request).await.expect("write routed rpc request");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read routed rpc response");
        decode_http_rpc_response(&response)
    }

    async fn shutdown(mut self) {
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }

    async fn disable_rate_limits(&self) {
        let request = encode_http_rpc_request(&RpcRequest {
            id: 900,
            method: "sdk_configure_v2".to_string(),
            params: Some(json!({
                "expected_revision": 0,
                "patch": {
                    "extensions": {
                        "rate_limits": {
                            "per_ip_per_minute": 0,
                            "per_principal_per_minute": 0
                        }
                    }
                }
            })),
        });
        let response = self.request(&request).await;
        assert!(response.error.is_none(), "disable rate limits failed: {:?}", response.error);
    }
}

async fn wait_for_tcp_rpc(addr: std::net::SocketAddr, child: &mut Child) {
    for _ in 0..200 {
        if let Ok(stream) = TcpStream::connect(addr).await {
            drop(stream);
            return;
        }
        if let Ok(Some(status)) = child.try_wait() {
            panic!("routed reticulumd process exited before rpc socket was ready: {status}");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("routed reticulumd rpc socket did not become ready");
}

fn unused_loopback_addr() -> std::net::SocketAddr {
    let listener =
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("bind tcp port");
    listener.local_addr().expect("local tcp addr")
}

fn encode_http_rpc_request(request: &RpcRequest) -> Vec<u8> {
    let body = codec::encode_frame(request).expect("encode rpc request");
    let mut http = Vec::new();
    http.extend_from_slice(b"POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: ");
    http.extend_from_slice(body.len().to_string().as_bytes());
    http.extend_from_slice(b"\r\n\r\n");
    http.extend_from_slice(&body);
    http
}

fn decode_http_rpc_response(response: &[u8]) -> rns_rpc::RpcResponse {
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        panic!("missing http response header end");
    };
    codec::decode_frame(&response[(header_end + 4)..]).expect("decode rpc response")
}

impl ControlRouterChild {
    async fn spawn() -> Self {
        let temp_dir = tempfile::tempdir().expect("control router temp dir");
        let db_path = temp_dir.path().join("reticulum.db");
        let mut child = Command::new(worker_executable())
            .arg("--control-router-stdio")
            .arg("--db")
            .arg(db_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn reticulumd control router process");
        let stdin = child.stdin.take().expect("control router stdin");
        let stdout = child.stdout.take().expect("control router stdout");
        Self { child, stdin, stdout, _temp_dir: temp_dir, next_sequence: 1 }
    }

    async fn request(&mut self, request: &RpcRequest) -> rns_rpc::RpcResponse {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        write_control_envelope(
            &mut self.stdin,
            &ControlEnvelope::request(sequence, request.clone()),
        )
        .await
        .expect("write control request");
        let envelope =
            read_control_envelope(&mut self.stdout).await.expect("read control response");
        assert_eq!(envelope.sequence, sequence);
        let ControlMessage::RpcResponse { response } = envelope.message else {
            panic!("expected rpc response");
        };
        response
    }

    async fn shutdown(mut self) {
        let _ = write_control_envelope(
            &mut self.stdin,
            &ControlEnvelope::new(0, ControlMessage::Shutdown),
        )
        .await;
        drop(self.stdin);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

impl WorkerChild {
    async fn spawn() -> Self {
        let mut child = Command::new(worker_executable())
            .arg("--worker-stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn reticulumd worker process");
        let stdin = child.stdin.take().expect("worker stdin");
        let stdout = child.stdout.take().expect("worker stdout");
        Self { child, stdin, stdout }
    }

    async fn submit(&mut self, request: &[u8]) -> WorkerResponse {
        write_worker_frame(&mut self.stdin, request, MAX_WORKER_REQUEST_BYTES)
            .await
            .expect("write worker request");
        let response = read_worker_frame(&mut self.stdout, MAX_WORKER_RESPONSE_BYTES)
            .await
            .expect("read worker response");
        WorkerResponse::decode(&response).expect("decode worker response")
    }

    async fn shutdown(mut self) {
        drop(self.stdin);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

fn bench_worker_local_resource_complete(c: &mut Criterion) {
    let payload = vec![0x5a; 4096];
    let job = unencrypted_resource_complete_job(1, &payload);
    c.bench_function("reticulumd/worker_local_resource_complete", |b| {
        b.iter(|| {
            let result = black_box(job.kind.clone())
                .complete_resource_with(|_| unreachable!("fixture is not encrypted"))
                .expect("resource completion");
            black_box(result);
        });
    });
}

fn bench_worker_local_outbound_encrypt(c: &mut Criterion) {
    let payload = vec![0x42; 256];
    let job = outbound_encrypt_job(11, &payload);
    c.bench_function("reticulumd/worker_local_outbound_encrypt", |b| {
        b.iter(|| {
            let packet_wire = complete_outbound_encrypt_locally(black_box(&job.kind));
            black_box(packet_wire);
        });
    });
}

fn bench_worker_stdio_outbound_encrypt_round_trip(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio runtime");
    let payload = vec![0x42; 256];
    let request =
        WorkerRequest::new(outbound_encrypt_job(12, &payload), 1_000).encode().expect("request");
    let mut worker = runtime.block_on(WorkerChild::spawn());

    c.bench_function("reticulumd/worker_stdio_outbound_encrypt_round_trip", |b| {
        b.iter_custom(|iters| {
            runtime.block_on(async {
                let started = Instant::now();
                for _ in 0..iters {
                    let response = worker.submit(black_box(&request)).await;
                    black_box(response.outcome.expect("worker response"));
                }
                started.elapsed()
            })
        });
    });

    runtime.block_on(worker.shutdown());
}

fn bench_worker_local_outbound_encrypt_batch_64(c: &mut Criterion) {
    let payload = vec![0x42; 256];
    let job = outbound_encrypt_batch_job(13, &payload, 64);
    c.bench_function("reticulumd/worker_local_outbound_encrypt_batch_64", |b| {
        b.iter(|| {
            let total = complete_outbound_encrypt_batch_locally(black_box(&job.kind));
            black_box(total);
        });
    });
}

fn bench_worker_stdio_outbound_encrypt_batch_64_round_trip(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio runtime");
    let payload = vec![0x42; 256];
    let request = WorkerRequest::new(outbound_encrypt_batch_job(14, &payload, 64), 5_000)
        .encode()
        .expect("request");
    let mut worker = runtime.block_on(WorkerChild::spawn());

    c.bench_function("reticulumd/worker_stdio_outbound_encrypt_batch_64_round_trip", |b| {
        b.iter_custom(|iters| {
            runtime.block_on(async {
                let started = Instant::now();
                for _ in 0..iters {
                    let response = worker.submit(black_box(&request)).await;
                    let result = response.outcome.expect("worker response");
                    let WorkerResultKind::PacketWireBatch { items } = result.kind else {
                        panic!("expected packet wire batch response");
                    };
                    black_box(items.len());
                }
                started.elapsed()
            })
        });
    });

    runtime.block_on(worker.shutdown());
}

fn bench_worker_stdio_resource_complete_round_trip(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio runtime");
    let payload = vec![0x5a; 4096];
    let request = WorkerRequest::new(unencrypted_resource_complete_job(2, &payload), 1_000)
        .encode()
        .expect("encode worker request");
    let mut worker = runtime.block_on(WorkerChild::spawn());

    c.bench_function("reticulumd/worker_stdio_resource_complete_round_trip", |b| {
        b.iter_custom(|iters| {
            runtime.block_on(async {
                let started = Instant::now();
                for _ in 0..iters {
                    let response = worker.submit(black_box(&request)).await;
                    black_box(response.outcome.expect("worker response"));
                }
                started.elapsed()
            })
        });
    });

    runtime.block_on(worker.shutdown());
}

fn bench_control_router_stdio_status_round_trip(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio runtime");
    let request = RpcRequest { id: 1, method: "daemon_status_ex".to_string(), params: None };
    let mut router = runtime.block_on(ControlRouterChild::spawn());

    c.bench_function("reticulumd/control_router_stdio_status_round_trip", |b| {
        b.iter_custom(|iters| {
            runtime.block_on(async {
                let started = Instant::now();
                for _ in 0..iters {
                    let response = router.request(black_box(&request)).await;
                    assert!(response.error.is_none());
                    black_box(response.result.expect("daemon status result"));
                }
                started.elapsed()
            })
        });
    });

    runtime.block_on(router.shutdown());
}

fn bench_control_router_http_status_routed_round_trip(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio runtime");
    let request =
        encode_http_rpc_request(&RpcRequest { id: 1, method: "status".to_string(), params: None });
    let daemon = runtime.block_on(RoutedHttpDaemon::spawn());

    c.bench_function("reticulumd/control_router_http_status_routed_round_trip", |b| {
        b.iter_custom(|iters| {
            runtime.block_on(async {
                let started = Instant::now();
                for _ in 0..iters {
                    let response = daemon.request(black_box(&request)).await;
                    assert!(response.error.is_none(), "routed status error: {:?}", response.error);
                    black_box(response.result.expect("daemon status result"));
                }
                started.elapsed()
            })
        });
    });

    runtime.block_on(daemon.shutdown());
}

criterion_group!(
    benches,
    bench_worker_local_resource_complete,
    bench_worker_local_outbound_encrypt,
    bench_worker_stdio_resource_complete_round_trip,
    bench_worker_stdio_outbound_encrypt_round_trip,
    bench_worker_local_outbound_encrypt_batch_64,
    bench_worker_stdio_outbound_encrypt_batch_64_round_trip,
    bench_control_router_stdio_status_round_trip,
    bench_control_router_http_status_routed_round_trip
);
criterion_main!(benches);
