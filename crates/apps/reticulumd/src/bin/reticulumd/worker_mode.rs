use std::net::{SocketAddr, TcpStream as StdTcpStream};
#[cfg(unix)]
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rand_core::OsRng;
use rns_transport::destination::DestinationAnnounce;
use rns_transport::hash::AddressHash;
use rns_transport::packet::{Packet, PacketDataBuffer};
use rns_transport::ratchets::encrypt_for_public_key_bytes;
use rns_transport::transport::worker_boundary::{
    read_worker_frame, write_worker_frame, DestinationPayloadBatchItem, PacketWireBatchItem,
    WorkerBackend, WorkerClient, WorkerCodecError, WorkerError, WorkerJob, WorkerJobFuture,
    WorkerJobKind, WorkerRequest, WorkerResponse, WorkerResult, WorkerResultKind,
    MAX_WORKER_REQUEST_BYTES, MAX_WORKER_RESPONSE_BYTES,
};
#[cfg(test)]
use tokio::io::DuplexStream;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;

#[allow(dead_code)]
pub(super) enum WorkerProcessEndpoint {
    Spawn {
        executable: PathBuf,
    },
    Tcp {
        addr: SocketAddr,
    },
    #[cfg(unix)]
    UnixSocket {
        path: PathBuf,
    },
    #[cfg(test)]
    InMemory {
        stream: Arc<std::sync::Mutex<Option<DuplexStream>>>,
    },
}

impl WorkerProcessEndpoint {
    fn spawn(executable: impl AsRef<Path>) -> Self {
        Self::Spawn { executable: executable.as_ref().to_path_buf() }
    }
}

#[allow(dead_code)]
enum WorkerProcessIo {
    Child {
        child: Child,
        stdin: Option<ChildStdin>,
        stdout: Option<ChildStdout>,
    },
    Tcp {
        stream: TcpStream,
    },
    #[cfg(unix)]
    UnixSocket {
        stream: UnixStream,
    },
    #[cfg(test)]
    InMemory {
        stream: DuplexStream,
    },
}

#[allow(dead_code)]
pub(super) struct WorkerStdioProcess {
    io: WorkerProcessIo,
}

#[allow(dead_code)]
impl WorkerStdioProcess {
    pub(super) fn spawn(executable: impl AsRef<Path>) -> Result<Self, WorkerProcessError> {
        Self::connect(&WorkerProcessEndpoint::spawn(executable))
    }

    fn connect(endpoint: &WorkerProcessEndpoint) -> Result<Self, WorkerProcessError> {
        match endpoint {
            WorkerProcessEndpoint::Spawn { executable } => Self::spawn_child(executable),
            WorkerProcessEndpoint::Tcp { addr } => Self::connect_tcp(*addr),
            #[cfg(unix)]
            WorkerProcessEndpoint::UnixSocket { path } => Self::connect_unix_socket(path),
            #[cfg(test)]
            WorkerProcessEndpoint::InMemory { stream } => Self::connect_in_memory(stream),
        }
    }

    fn spawn_child(executable: &Path) -> Result<Self, WorkerProcessError> {
        let mut child = Command::new(executable)
            .arg("--worker-stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| WorkerProcessError::Spawn {
                executable: executable.to_path_buf(),
                message: err.to_string(),
            })?;
        let stdin =
            child.stdin.take().ok_or_else(|| WorkerProcessError::MissingPipe { name: "stdin" })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| WorkerProcessError::MissingPipe { name: "stdout" })?;
        Ok(Self { io: WorkerProcessIo::Child { child, stdin: Some(stdin), stdout: Some(stdout) } })
    }

    fn connect_tcp(addr: SocketAddr) -> Result<Self, WorkerProcessError> {
        let stream = StdTcpStream::connect(addr)
            .map_err(|err| WorkerProcessError::ConnectTcp { addr, message: err.to_string() })?;
        stream
            .set_nonblocking(true)
            .map_err(|err| WorkerProcessError::ConnectTcp { addr, message: err.to_string() })?;
        let stream = TcpStream::from_std(stream)
            .map_err(|err| WorkerProcessError::ConnectTcp { addr, message: err.to_string() })?;
        Ok(Self { io: WorkerProcessIo::Tcp { stream } })
    }

    #[cfg(test)]
    fn connect_in_memory(
        stream: &Arc<std::sync::Mutex<Option<DuplexStream>>>,
    ) -> Result<Self, WorkerProcessError> {
        let stream = stream
            .lock()
            .map_err(|_| WorkerProcessError::InvalidConfig {
                message: "in-memory worker stream lock poisoned".to_string(),
            })?
            .take()
            .ok_or_else(|| WorkerProcessError::InvalidConfig {
                message: "in-memory worker stream already consumed".to_string(),
            })?;
        Ok(Self { io: WorkerProcessIo::InMemory { stream } })
    }

    #[cfg(test)]
    fn child_id(&self) -> Option<u32> {
        match &self.io {
            WorkerProcessIo::Child { child, .. } => child.id(),
            WorkerProcessIo::Tcp { .. } => None,
            #[cfg(unix)]
            WorkerProcessIo::UnixSocket { .. } => None,
            WorkerProcessIo::InMemory { .. } => None,
        }
    }

    #[cfg(unix)]
    fn connect_unix_socket(path: &Path) -> Result<Self, WorkerProcessError> {
        let stream = StdUnixStream::connect(path).map_err(|err| {
            WorkerProcessError::ConnectSocket { path: path.to_path_buf(), message: err.to_string() }
        })?;
        stream.set_nonblocking(true).map_err(|err| WorkerProcessError::ConnectSocket {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        let stream = UnixStream::from_std(stream).map_err(|err| {
            WorkerProcessError::ConnectSocket { path: path.to_path_buf(), message: err.to_string() }
        })?;
        Ok(Self { io: WorkerProcessIo::UnixSocket { stream } })
    }

    pub(super) async fn submit_encoded(
        &mut self,
        request: &[u8],
    ) -> Result<Vec<u8>, WorkerProcessError> {
        match &mut self.io {
            WorkerProcessIo::Child { stdin, stdout, .. } => {
                let stdin =
                    stdin.as_mut().ok_or(WorkerProcessError::ClosedPipe { name: "stdin" })?;
                let stdout =
                    stdout.as_mut().ok_or(WorkerProcessError::ClosedPipe { name: "stdout" })?;
                write_worker_frame(stdin, request, MAX_WORKER_REQUEST_BYTES)
                    .await
                    .map_err(WorkerProcessError::Write)?;
                read_worker_frame(stdout, MAX_WORKER_RESPONSE_BYTES)
                    .await
                    .map_err(WorkerProcessError::Read)
            }
            WorkerProcessIo::Tcp { stream } => {
                write_worker_frame(&mut *stream, request, MAX_WORKER_REQUEST_BYTES)
                    .await
                    .map_err(WorkerProcessError::Write)?;
                read_worker_frame(&mut *stream, MAX_WORKER_RESPONSE_BYTES)
                    .await
                    .map_err(WorkerProcessError::Read)
            }
            #[cfg(test)]
            WorkerProcessIo::InMemory { stream } => {
                write_worker_frame(&mut *stream, request, MAX_WORKER_REQUEST_BYTES)
                    .await
                    .map_err(WorkerProcessError::Write)?;
                read_worker_frame(&mut *stream, MAX_WORKER_RESPONSE_BYTES)
                    .await
                    .map_err(WorkerProcessError::Read)
            }
            #[cfg(unix)]
            WorkerProcessIo::UnixSocket { stream } => {
                write_worker_frame(&mut *stream, request, MAX_WORKER_REQUEST_BYTES)
                    .await
                    .map_err(WorkerProcessError::Write)?;
                read_worker_frame(&mut *stream, MAX_WORKER_RESPONSE_BYTES)
                    .await
                    .map_err(WorkerProcessError::Read)
            }
        }
    }

    pub(super) async fn shutdown(
        mut self,
        wait: Duration,
    ) -> Result<ExitStatus, WorkerProcessError> {
        match &mut self.io {
            WorkerProcessIo::Child { child, stdin, .. } => {
                drop(stdin.take());
                timeout(wait, child.wait())
                    .await
                    .map_err(|_| WorkerProcessError::ShutdownTimedOut)?
                    .map_err(|err| WorkerProcessError::Wait { message: err.to_string() })
            }
            WorkerProcessIo::Tcp { .. } => Err(WorkerProcessError::ExternalWorkerNoExitStatus),
            #[cfg(test)]
            WorkerProcessIo::InMemory { .. } => Err(WorkerProcessError::ExternalWorkerNoExitStatus),
            #[cfg(unix)]
            WorkerProcessIo::UnixSocket { .. } => {
                Err(WorkerProcessError::ExternalWorkerNoExitStatus)
            }
        }
    }
}

impl Drop for WorkerStdioProcess {
    fn drop(&mut self) {
        if let WorkerProcessIo::Child { child, .. } = &mut self.io {
            let _ = child.start_kill();
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(super) enum WorkerProcessError {
    EmptyPool,
    InvalidConfig {
        message: String,
    },
    Spawn {
        executable: PathBuf,
        message: String,
    },
    ConnectTcp {
        addr: SocketAddr,
        message: String,
    },
    #[cfg(unix)]
    ConnectSocket {
        path: PathBuf,
        message: String,
    },
    MissingPipe {
        name: &'static str,
    },
    ClosedPipe {
        name: &'static str,
    },
    Write(WorkerCodecError),
    Read(WorkerCodecError),
    Wait {
        message: String,
    },
    ShutdownTimedOut,
    ExternalWorkerNoExitStatus,
    RequestTimedOut {
        timeout_ms: u64,
    },
}

pub(super) fn validate_worker_process_options(
    worker_count: usize,
    timeout_ms: u64,
) -> Result<(), WorkerProcessError> {
    if worker_count == 0 {
        return Ok(());
    }
    if timeout_ms == 0 {
        return Err(WorkerProcessError::InvalidConfig {
            message: "worker process timeout must be greater than zero when workers are enabled"
                .to_string(),
        });
    }
    Ok(())
}

#[allow(dead_code)]
pub(super) struct WorkerStdioPool {
    endpoint: WorkerProcessEndpoint,
    workers: Vec<Mutex<WorkerStdioProcess>>,
    next: AtomicUsize,
    request_timeouts: AtomicUsize,
    child_replacements: AtomicUsize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WorkerStdioPoolSnapshot {
    pub(super) worker_count: usize,
    pub(super) idle_workers: usize,
    pub(super) busy_workers: usize,
    pub(super) request_timeouts: usize,
    pub(super) child_replacements: usize,
}

#[allow(dead_code)]
impl WorkerStdioPool {
    pub(super) fn spawn(
        executable: impl AsRef<Path>,
        worker_count: usize,
    ) -> Result<Self, WorkerProcessError> {
        Self::connect(WorkerProcessEndpoint::spawn(executable), worker_count)
    }

    pub(super) fn connect(
        endpoint: WorkerProcessEndpoint,
        worker_count: usize,
    ) -> Result<Self, WorkerProcessError> {
        if worker_count == 0 {
            return Err(WorkerProcessError::EmptyPool);
        }

        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            workers.push(Mutex::new(WorkerStdioProcess::connect(&endpoint)?));
        }

        Ok(Self {
            endpoint,
            workers,
            next: AtomicUsize::new(0),
            request_timeouts: AtomicUsize::new(0),
            child_replacements: AtomicUsize::new(0),
        })
    }

    pub(super) async fn submit_encoded(
        &self,
        request: &[u8],
        wait: Duration,
    ) -> Result<Vec<u8>, WorkerProcessError> {
        let (_index, mut worker) = self.lock_worker_for_submit().await;
        match timeout(wait, worker.submit_encoded(request)).await {
            Ok(result) => result,
            Err(_) => {
                let timeout_ms = wait.as_millis().min(u128::from(u64::MAX)) as u64;
                self.request_timeouts.fetch_add(1, Ordering::Relaxed);
                if let WorkerProcessIo::Child { child, .. } = &mut worker.io {
                    let _ = child.start_kill();
                }
                let replacement = WorkerStdioProcess::connect(&self.endpoint)?;
                let _timed_out = std::mem::replace(&mut *worker, replacement);
                self.child_replacements.fetch_add(1, Ordering::Relaxed);
                Err(WorkerProcessError::RequestTimedOut { timeout_ms })
            }
        }
    }

    pub(super) fn snapshot(&self) -> WorkerStdioPoolSnapshot {
        let idle_workers = self.workers.iter().filter_map(|worker| worker.try_lock().ok()).count();
        let worker_count = self.workers.len();
        WorkerStdioPoolSnapshot {
            worker_count,
            idle_workers,
            busy_workers: worker_count.saturating_sub(idle_workers),
            request_timeouts: self.request_timeouts.load(Ordering::Relaxed),
            child_replacements: self.child_replacements.load(Ordering::Relaxed),
        }
    }

    async fn lock_worker_for_submit(
        &self,
    ) -> (usize, tokio::sync::MutexGuard<'_, WorkerStdioProcess>) {
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
    ) -> Vec<Result<ExitStatus, WorkerProcessError>> {
        let mut results = Vec::with_capacity(self.workers.len());
        for worker in self.workers {
            results.push(worker.into_inner().shutdown(wait).await);
        }
        results
    }
}

#[allow(dead_code)]
pub(super) struct WorkerStdioPoolBackend {
    pool: Arc<WorkerStdioPool>,
    timeout_ms: u64,
}

#[allow(dead_code)]
impl WorkerStdioPoolBackend {
    pub(super) fn new(pool: WorkerStdioPool, timeout_ms: u64) -> Self {
        Self { pool: Arc::new(pool), timeout_ms }
    }

    pub(super) fn snapshot(&self) -> WorkerStdioPoolSnapshot {
        self.pool.snapshot()
    }
}

impl WorkerBackend for WorkerStdioPoolBackend {
    fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
        let pool = Arc::clone(&self.pool);
        let timeout_ms = self.timeout_ms;
        Box::pin(async move {
            let request = WorkerRequest::new(job, timeout_ms);
            let request = request.encode().map_err(|err| WorkerError::InvalidJob {
                message: format!("failed to encode worker process request: {err:?}"),
            })?;
            let response = pool
                .submit_encoded(&request, Duration::from_millis(timeout_ms))
                .await
                .map_err(|err| WorkerError::BackendUnavailable {
                message: format!("worker process pool request failed: {err:?}"),
            })?;
            let response =
                WorkerResponse::decode(&response).map_err(|err| WorkerError::InvalidJob {
                    message: format!("failed to decode worker process response: {err:?}"),
                })?;
            response.outcome
        })
    }
}

#[allow(dead_code)]
pub(super) fn spawn_worker_process_backend(
    executable: impl AsRef<Path>,
    worker_count: usize,
    timeout_ms: u64,
) -> Result<Arc<WorkerStdioPoolBackend>, WorkerProcessError> {
    spawn_worker_process_backend_from_endpoint(
        WorkerProcessEndpoint::spawn(executable),
        worker_count,
        timeout_ms,
    )
}

pub(super) fn spawn_worker_process_backend_from_endpoint(
    endpoint: WorkerProcessEndpoint,
    worker_count: usize,
    timeout_ms: u64,
) -> Result<Arc<WorkerStdioPoolBackend>, WorkerProcessError> {
    validate_worker_process_options(worker_count, timeout_ms)?;
    if worker_count == 0 {
        return Err(WorkerProcessError::EmptyPool);
    }
    let pool = WorkerStdioPool::connect(endpoint, worker_count)?;
    Ok(Arc::new(WorkerStdioPoolBackend::new(pool, timeout_ms)))
}

struct StdioWorkerBackend;

impl WorkerBackend for StdioWorkerBackend {
    fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
        Box::pin(async move {
            match job.kind {
                WorkerJobKind::ValidateAnnounce { packet_wire } => {
                    validate_announce_job(job.id, packet_wire)
                }
                WorkerJobKind::OutboundEncrypt { packet_wire, public_key, salt } => {
                    outbound_encrypt_job(job.id, packet_wire, public_key, salt)
                }
                WorkerJobKind::OutboundEncryptBatch { items } => {
                    outbound_encrypt_batch_job(job.id, items)
                }
                WorkerJobKind::SingleDestinationDecrypt {
                    packet_wire,
                    destination,
                    private_key,
                } => single_destination_decrypt_job(job.id, packet_wire, destination, private_key),
                WorkerJobKind::SingleDestinationDecryptBatch { items } => {
                    single_destination_decrypt_batch_job(job.id, items)
                }
                kind @ WorkerJobKind::ResourceComplete { .. } => {
                    resource_complete_job(job.id, kind)
                }
                _ => Err(WorkerError::BackendUnavailable {
                    message: format!(
                        "reticulumd worker stdio backend is not wired for job {} yet",
                        job.id
                    ),
                }),
            }
        })
    }
}

fn outbound_encrypt_job(
    job_id: u64,
    packet_wire: Vec<u8>,
    public_key: [u8; rns_transport::identity::PUBLIC_KEY_LENGTH],
    salt: [u8; rns_transport::hash::ADDRESS_HASH_SIZE],
) -> Result<WorkerResult, WorkerError> {
    let mut packet = Packet::from_bytes(packet_wire.as_slice()).map_err(|err| {
        WorkerError::Packet { message: format!("failed to decode outbound packet: {err:?}") }
    })?;
    let ciphertext =
        encrypt_for_public_key_bytes(&public_key, &salt, packet.data.as_slice(), OsRng).map_err(
            |err| WorkerError::Crypto { message: format!("outbound encrypt failed: {err:?}") },
        )?;
    let mut buffer = PacketDataBuffer::new();
    buffer.write(&ciphertext).map_err(|err| WorkerError::Packet {
        message: format!("encrypted packet is too large: {err:?}"),
    })?;
    packet.data = buffer;
    let packet_wire = packet.to_bytes().map_err(|err| WorkerError::Packet {
        message: format!("failed to encode encrypted packet: {err:?}"),
    })?;
    Ok(WorkerResult { id: job_id, kind: WorkerResultKind::PacketWire { packet_wire } })
}

fn outbound_encrypt_batch_job(
    job_id: u64,
    items: Vec<rns_transport::transport::worker_boundary::OutboundEncryptBatchItem>,
) -> Result<WorkerResult, WorkerError> {
    let items = parallel_map_worker_items(items, |item| {
        let result = outbound_encrypt_job(job_id, item.packet_wire, item.public_key, item.salt)?;
        let WorkerResultKind::PacketWire { packet_wire } = result.kind else {
            return Err(WorkerError::InvalidJob {
                message: "outbound encrypt item returned unexpected result".to_string(),
            });
        };
        Ok(PacketWireBatchItem { packet_wire })
    })?;
    Ok(WorkerResult { id: job_id, kind: WorkerResultKind::PacketWireBatch { items } })
}

fn single_destination_decrypt_job(
    job_id: u64,
    packet_wire: Vec<u8>,
    destination: [u8; rns_transport::hash::ADDRESS_HASH_SIZE],
    private_key: serde_bytes::ByteBuf,
) -> Result<WorkerResult, WorkerError> {
    let packet = Packet::from_bytes(packet_wire.as_slice()).map_err(|err| WorkerError::Packet {
        message: format!("failed to decode single destination packet: {err:?}"),
    })?;
    if packet.destination.as_slice() != destination {
        return Err(WorkerError::InvalidJob {
            message: "single destination decrypt job destination mismatch".to_string(),
        });
    }
    let identity = rns_transport::identity::PrivateIdentity::from_private_key_bytes(&private_key)
        .map_err(|err| WorkerError::InvalidJob {
        message: format!("invalid private identity bytes: {err:?}"),
    })?;
    let salt = identity.as_identity().address_hash;
    let payload = rns_transport::ratchets::decrypt_with_identity(
        &identity,
        salt.as_slice(),
        packet.data.as_slice(),
    )
    .map_err(|err| WorkerError::Crypto {
        message: format!("single destination decrypt failed: {err:?}"),
    })?;
    Ok(WorkerResult {
        id: job_id,
        kind: WorkerResultKind::DestinationPayload {
            payload: serde_bytes::ByteBuf::from(payload),
            ratchet_used: false,
        },
    })
}

fn single_destination_decrypt_batch_job(
    job_id: u64,
    items: Vec<rns_transport::transport::worker_boundary::SingleDestinationDecryptBatchItem>,
) -> Result<WorkerResult, WorkerError> {
    let items = parallel_map_worker_items(items, |item| {
        let result = single_destination_decrypt_job(
            job_id,
            item.packet_wire,
            item.destination,
            item.private_key,
        )?;
        let WorkerResultKind::DestinationPayload { payload, ratchet_used } = result.kind else {
            return Err(WorkerError::InvalidJob {
                message: "single destination decrypt item returned unexpected result".to_string(),
            });
        };
        Ok(DestinationPayloadBatchItem { payload, ratchet_used })
    })?;
    Ok(WorkerResult { id: job_id, kind: WorkerResultKind::DestinationPayloadBatch { items } })
}

fn parallel_map_worker_items<I, O, F>(items: Vec<I>, map: F) -> Result<Vec<O>, WorkerError>
where
    I: Send,
    O: Send,
    F: Fn(I) -> Result<O, WorkerError> + Sync,
{
    if items.len() <= 1 {
        return items.into_iter().map(map).collect();
    }

    let workers = thread::available_parallelism().map_or(1, usize::from).clamp(1, items.len());
    let mut chunks = (0..workers).map(|_| Vec::new()).collect::<Vec<_>>();
    for (index, item) in items.into_iter().enumerate() {
        chunks[index % workers].push((index, item));
    }
    let mut mapped = thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in chunks {
            let map = &map;
            handles.push(scope.spawn(move || {
                let mut mapped = Vec::with_capacity(chunk.len());
                for (index, item) in chunk {
                    mapped.push((index, map(item)?));
                }
                Ok::<_, WorkerError>(mapped)
            }));
        }

        let mut mapped = Vec::new();
        for handle in handles {
            mapped.extend(handle.join().map_err(|_| WorkerError::BackendUnavailable {
                message: "batch worker thread panicked".to_string(),
            })??);
        }
        Ok::<_, WorkerError>(mapped)
    })?;
    mapped.sort_by_key(|(index, _)| *index);
    Ok(mapped.into_iter().map(|(_, item)| item).collect())
}

fn resource_complete_job(job_id: u64, kind: WorkerJobKind) -> Result<WorkerResult, WorkerError> {
    let kind = kind.complete_resource_with(|_| {
        Err(WorkerError::BackendUnavailable {
            message: "encrypted resource completion requires link decrypt context".to_string(),
        })
    })?;
    Ok(WorkerResult { id: job_id, kind })
}

fn validate_announce_job(job_id: u64, packet_wire: Vec<u8>) -> Result<WorkerResult, WorkerError> {
    let packet = Packet::from_bytes(packet_wire.as_slice()).map_err(|err| WorkerError::Packet {
        message: format!("failed to decode announce packet: {err:?}"),
    })?;
    let info = DestinationAnnounce::validate(&packet).map_err(|err| WorkerError::Packet {
        message: format!("announce validation failed: {err:?}"),
    })?;
    Ok(WorkerResult {
        id: job_id,
        kind: WorkerResultKind::AnnounceValidated {
            destination: address_hash_bytes(&info.destination.desc.address_hash),
            public_key: *info.destination.desc.identity.public_key.as_bytes(),
            verifying_key: *info.destination.desc.identity.verifying_key.as_bytes(),
            name_hash: name_hash_bytes(info.destination.desc.name.as_name_hash_slice()),
            app_data: serde_bytes::ByteBuf::from(info.app_data.to_vec()),
            ratchet: info.ratchet.map(|ratchet| serde_bytes::ByteBuf::from(ratchet.to_vec())),
        },
    })
}

fn address_hash_bytes(hash: &AddressHash) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(hash.as_slice());
    bytes
}

fn name_hash_bytes(hash: &[u8]) -> [u8; rns_transport::destination::NAME_HASH_LENGTH] {
    let mut bytes = [0u8; rns_transport::destination::NAME_HASH_LENGTH];
    bytes.copy_from_slice(hash);
    bytes
}

pub(super) async fn run_worker_stdio() {
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    run_worker_stream(&mut stdin, &mut stdout).await;
}

async fn run_worker_stream<R, W>(reader: &mut R, writer: &mut W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let client = WorkerClient::new(Arc::new(StdioWorkerBackend));
    let mut handled = 0usize;

    loop {
        let request = match read_worker_frame(reader, MAX_WORKER_REQUEST_BYTES).await {
            Ok(request) => request,
            Err(WorkerCodecError::Io { message })
                if message.contains("early eof")
                    || message.contains("unexpected end of file")
                    || message.contains("operation interrupted") =>
            {
                eprintln!("[worker-stdio] stopped handled={handled} reason=eof");
                return;
            }
            Err(err) => {
                eprintln!("[worker-stdio] stopped handled={handled} err={err:?}");
                return;
            }
        };

        let response = client.submit_encoded(&request).await;
        if let Err(err) = write_worker_frame(writer, &response, MAX_WORKER_RESPONSE_BYTES).await {
            eprintln!("[worker-stdio] stopped handled={handled} err={err:?}");
            return;
        }
        handled = handled.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;
    use rns_transport::destination::{DestinationName, SingleInputDestination};
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::packet::{DestinationType, Header, PacketType};
    use rns_transport::transport::worker_boundary::{
        encode_worker_frame, read_worker_frame, write_worker_frame, WorkerRequest, WorkerResponse,
    };
    use sha2::Digest;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    const STALLED_WORKER_TIMEOUT_MS: u64 = 2_000;

    fn announce_worker_request(id: u64) -> (WorkerRequest, SingleInputDestination) {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let mut destination =
            SingleInputDestination::new(identity, DestinationName::new("lxmf", "delivery"));
        let app_data = b"worker announce";
        let announce = destination.announce(OsRng, Some(app_data)).expect("announce");
        let packet_wire = announce.to_bytes().expect("announce wire");
        (
            WorkerRequest::new(
                WorkerJob { id, kind: WorkerJobKind::ValidateAnnounce { packet_wire } },
                100,
            ),
            destination,
        )
    }

    fn unencrypted_resource_complete_job(payload: &[u8]) -> WorkerJobKind {
        let random_hash = [0x5a; rns_transport::resource::RANDOM_HASH_SIZE];
        let mut hasher = sha2::Sha256::new();
        hasher.update(payload);
        hasher.update(random_hash);
        let digest = hasher.finalize();
        let mut resource_hash = [0u8; rns_transport::hash::HASH_SIZE];
        resource_hash.copy_from_slice(&digest[..rns_transport::hash::HASH_SIZE]);
        let mut stream = random_hash.to_vec();
        stream.extend_from_slice(payload);

        WorkerJobKind::ResourceComplete {
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
        }
    }

    #[tokio::test]
    async fn stdio_worker_validates_announce_jobs() {
        let app_data = b"worker announce";
        let (request, destination) = announce_worker_request(1);
        let client = WorkerClient::new(Arc::new(StdioWorkerBackend));
        let request = request.encode().expect("encode request");

        let response = client.submit_encoded(&request).await;
        let response = WorkerResponse::decode(&response).expect("decode response");

        assert_eq!(response.job_id, 1);
        let result = response.outcome.expect("announce validation should succeed");
        assert_eq!(result.id, 1);
        let WorkerResultKind::AnnounceValidated {
            destination: validated_destination,
            public_key,
            verifying_key,
            name_hash,
            app_data: validated_app_data,
            ratchet,
        } = result.kind
        else {
            panic!("unexpected worker result kind");
        };
        assert_eq!(validated_destination, address_hash_bytes(&destination.desc.address_hash));
        assert_eq!(public_key, *destination.desc.identity.public_key.as_bytes());
        assert_eq!(verifying_key, *destination.desc.identity.verifying_key.as_bytes());
        assert_eq!(name_hash, name_hash_bytes(destination.desc.name.as_name_hash_slice()));
        assert_eq!(validated_app_data.as_ref(), app_data);
        assert!(ratchet.is_none());
    }

    #[tokio::test]
    async fn stdio_worker_stream_processes_framed_announce_request() {
        let app_data = b"worker announce";
        let (request, destination) = announce_worker_request(3);
        let request = request.encode().expect("encode request");
        let (mut caller_tx, mut worker_rx) = tokio::io::duplex(512);
        let (mut worker_tx, mut caller_rx) = tokio::io::duplex(512);

        let worker = tokio::spawn(async move {
            run_worker_stream(&mut worker_rx, &mut worker_tx).await;
        });

        write_worker_frame(&mut caller_tx, &request, MAX_WORKER_REQUEST_BYTES)
            .await
            .expect("write request");
        let response = read_worker_frame(&mut caller_rx, MAX_WORKER_RESPONSE_BYTES)
            .await
            .expect("read response");
        drop(caller_tx);
        let response = WorkerResponse::decode(&response).expect("decode response");

        assert_eq!(response.job_id, 3);
        let result = response.outcome.expect("announce validation should succeed");
        let WorkerResultKind::AnnounceValidated {
            destination: validated_destination,
            public_key,
            verifying_key,
            name_hash,
            app_data: validated_app_data,
            ratchet,
        } = result.kind
        else {
            panic!("unexpected worker result kind");
        };
        assert_eq!(validated_destination, address_hash_bytes(&destination.desc.address_hash));
        assert_eq!(public_key, *destination.desc.identity.public_key.as_bytes());
        assert_eq!(verifying_key, *destination.desc.identity.verifying_key.as_bytes());
        assert_eq!(name_hash, name_hash_bytes(destination.desc.name.as_name_hash_slice()));
        assert_eq!(validated_app_data.as_ref(), app_data);
        assert!(ratchet.is_none());

        worker.await.expect("worker task");
    }

    #[tokio::test]
    async fn stdio_worker_returns_backend_unavailable_for_unwired_jobs() {
        let client = WorkerClient::new(Arc::new(StdioWorkerBackend));
        let request = WorkerRequest::new(
            WorkerJob {
                id: 2,
                kind: WorkerJobKind::ResourcePrepare {
                    link_id: [0x11; 16],
                    data: b"resource".to_vec(),
                    metadata: None,
                    request_id: None,
                    is_response: false,
                },
            },
            100,
        )
        .encode()
        .expect("encode request");

        let response = client.submit_encoded(&request).await;
        let response = WorkerResponse::decode(&response).expect("decode response");

        assert_eq!(response.job_id, 2);
        assert!(matches!(response.outcome, Err(WorkerError::BackendUnavailable { .. })));
    }

    #[tokio::test]
    async fn stdio_worker_encrypts_outbound_packets() {
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
            data: PacketDataBuffer::new_from_slice(b"plain outbound"),
            ..Default::default()
        };
        let client = WorkerClient::new(Arc::new(StdioWorkerBackend));
        let request = WorkerRequest::new(
            WorkerJob {
                id: 4,
                kind: WorkerJobKind::OutboundEncrypt {
                    packet_wire: packet.to_bytes().expect("packet wire"),
                    public_key: *destination.desc.identity.public_key.as_bytes(),
                    salt: address_hash_bytes(&destination.desc.identity.address_hash),
                },
            },
            100,
        )
        .encode()
        .expect("encode request");

        let response = client.submit_encoded(&request).await;
        let response = WorkerResponse::decode(&response).expect("decode response");

        assert_eq!(response.job_id, 4);
        let result = response.outcome.expect("outbound encryption should succeed");
        let WorkerResultKind::PacketWire { packet_wire } = result.kind else {
            panic!("unexpected worker result kind");
        };
        let encrypted = Packet::from_bytes(&packet_wire).expect("encrypted packet");
        assert_eq!(encrypted.destination, packet.destination);
        assert_ne!(encrypted.data.as_slice(), b"plain outbound");
    }

    #[tokio::test]
    async fn stdio_worker_decrypts_single_destination_packets() {
        let local_identity = PrivateIdentity::new_from_rand(OsRng);
        let destination =
            SingleInputDestination::new(local_identity, DestinationName::new("lxmf", "delivery"));
        let salt = destination.identity.as_identity().address_hash;
        let ciphertext = encrypt_for_public_key_bytes(
            destination.desc.identity.public_key.as_bytes(),
            salt.as_slice(),
            b"plain inbound",
            OsRng,
        )
        .expect("encrypt inbound");
        let packet = Packet {
            header: Header {
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            destination: destination.desc.address_hash,
            data: PacketDataBuffer::new_from_slice(&ciphertext),
            ..Default::default()
        };
        let client = WorkerClient::new(Arc::new(StdioWorkerBackend));
        let request = WorkerRequest::new(
            WorkerJob {
                id: 5,
                kind: WorkerJobKind::SingleDestinationDecrypt {
                    packet_wire: packet.to_bytes().expect("packet wire"),
                    destination: address_hash_bytes(&destination.desc.address_hash),
                    private_key: serde_bytes::ByteBuf::from(
                        destination.identity.to_private_key_bytes().to_vec(),
                    ),
                },
            },
            100,
        )
        .encode()
        .expect("encode request");

        let response = client.submit_encoded(&request).await;
        let response = WorkerResponse::decode(&response).expect("decode response");

        assert_eq!(response.job_id, 5);
        let result = response.outcome.expect("single destination decrypt should succeed");
        let WorkerResultKind::DestinationPayload { payload, ratchet_used } = result.kind else {
            panic!("unexpected worker result kind");
        };
        assert_eq!(payload.as_ref(), b"plain inbound");
        assert!(!ratchet_used);
    }

    #[tokio::test]
    async fn stdio_worker_completes_unencrypted_resource_jobs() {
        let client = WorkerClient::new(Arc::new(StdioWorkerBackend));
        let request = WorkerRequest::new(
            WorkerJob { id: 6, kind: unencrypted_resource_complete_job(b"resource payload") },
            100,
        )
        .encode()
        .expect("encode request");

        let response = client.submit_encoded(&request).await;
        let response = WorkerResponse::decode(&response).expect("decode response");

        assert_eq!(response.job_id, 6);
        let result = response.outcome.expect("resource completion should succeed");
        let WorkerResultKind::ResourceCompleted {
            data,
            metadata,
            request_id,
            is_request,
            is_response,
            ..
        } = result.kind
        else {
            panic!("unexpected worker result kind");
        };
        assert_eq!(data.as_ref(), b"resource payload");
        assert!(metadata.is_none());
        assert!(request_id.is_none());
        assert!(!is_request);
        assert!(!is_response);
    }

    #[test]
    fn worker_process_spawn_reports_invalid_executable() {
        let Err(err) = WorkerStdioProcess::spawn("/definitely/not/a/reticulumd") else {
            panic!("invalid executable should fail");
        };

        let WorkerProcessError::Spawn { executable, message } = err else {
            panic!("unexpected process error");
        };
        assert_eq!(executable, PathBuf::from("/definitely/not/a/reticulumd"));
        assert!(!message.is_empty());
    }

    #[test]
    fn worker_process_pool_rejects_zero_workers() {
        let Err(err) = WorkerStdioPool::spawn("/definitely/not/a/reticulumd", 0) else {
            panic!("zero-worker pool should fail");
        };
        assert!(matches!(err, WorkerProcessError::EmptyPool));
    }

    #[tokio::test]
    async fn worker_process_pool_can_use_non_child_stream_worker() {
        let (client_stream, mut server_stream) = tokio::io::duplex(16 * 1024);
        let client_stream = Arc::new(std::sync::Mutex::new(Some(client_stream)));
        let server = tokio::spawn(async move {
            let frame = read_worker_frame(&mut server_stream, MAX_WORKER_REQUEST_BYTES)
                .await
                .expect("read worker request");
            let request = WorkerRequest::decode(&frame).expect("decode worker request");
            let response = WorkerResponse::success(WorkerResult {
                id: request.job.id,
                kind: WorkerResultKind::PacketWire { packet_wire: vec![1, 2, 3] },
            })
            .encode()
            .expect("encode worker response");
            write_worker_frame(&mut server_stream, &response, MAX_WORKER_RESPONSE_BYTES)
                .await
                .expect("write worker response");
        });

        let pool =
            WorkerStdioPool::connect(WorkerProcessEndpoint::InMemory { stream: client_stream }, 1)
                .expect("connect worker pool");
        let request = WorkerRequest::new(
            WorkerJob { id: 44, kind: WorkerJobKind::ValidateAnnounce { packet_wire: Vec::new() } },
            1_000,
        )
        .encode()
        .expect("encode worker request");

        let response = pool
            .submit_encoded(&request, Duration::from_millis(1_000))
            .await
            .expect("submit worker request");
        let response = WorkerResponse::decode(&response).expect("decode worker response");
        assert_eq!(response.job_id, 44);
        let result = response.outcome.expect("worker success");
        assert!(matches!(
            result.kind,
            WorkerResultKind::PacketWire { packet_wire } if packet_wire == vec![1, 2, 3]
        ));
        server.await.expect("server task");
    }

    #[test]
    fn worker_process_options_allow_disabled_pool_with_zero_timeout() {
        validate_worker_process_options(0, 0).expect("disabled worker pool should ignore timeout");
    }

    #[test]
    fn worker_process_options_reject_enabled_pool_with_zero_timeout() {
        let err = validate_worker_process_options(1, 0).expect_err("zero timeout should fail");
        assert!(matches!(err, WorkerProcessError::InvalidConfig { .. }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn worker_process_pool_times_out_and_replaces_stalled_child() {
        let temp = tempfile::tempdir().expect("tempdir");
        let marker = temp.path().join("first-worker-started");
        let script = temp.path().join("stalled-then-healthy-worker.py");
        let response = WorkerResponse::success(WorkerResult {
            id: 43,
            kind: WorkerResultKind::ResourceCompleted {
                resource_hash: [0x22; rns_transport::hash::HASH_SIZE],
                proof: [0x33; rns_transport::hash::HASH_SIZE],
                data: serde_bytes::ByteBuf::from(b"replacement response".to_vec()),
                metadata: None,
                request_id: None,
                is_request: false,
                is_response: false,
            },
        })
        .encode()
        .expect("encode replacement response");
        let response_frame =
            encode_worker_frame(&response, MAX_WORKER_RESPONSE_BYTES).expect("response frame");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import os
import struct
import sys
import time

marker = {marker:?}
if not os.path.exists(marker):
    open(marker, "w").close()
    time.sleep(5)
    sys.exit(0)

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
sys.stdout.buffer.write(bytes.fromhex({response_frame_hex:?}))
sys.stdout.buffer.flush()
"#,
                marker = marker.to_string_lossy(),
                response_frame_hex = hex::encode(response_frame),
            ),
        )
        .expect("write worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = WorkerStdioPool::spawn(&script, 1).expect("spawn stalled worker");
        let original_child_id = pool.workers[0].lock().await.child_id().expect("child process id");
        let marker_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !marker.exists() && tokio::time::Instant::now() < marker_deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(marker.exists(), "stalled worker should create marker before timeout test");
        let request = WorkerRequest::new(
            WorkerJob {
                id: 42,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            },
            10,
        )
        .encode()
        .expect("encode request");

        let err = pool
            .submit_encoded(&request, Duration::from_millis(10))
            .await
            .expect_err("stalled worker should time out");

        assert!(matches!(err, WorkerProcessError::RequestTimedOut { timeout_ms: 10 }));
        let replacement_child_id =
            pool.workers[0].lock().await.child_id().expect("child process id");
        assert_ne!(replacement_child_id, original_child_id);

        let next_request = WorkerRequest::new(
            WorkerJob {
                id: 43,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            },
            1_000,
        )
        .encode()
        .expect("encode next request");
        let response = pool
            .submit_encoded(&next_request, Duration::from_secs(1))
            .await
            .expect("replacement worker should serve next request");
        let response = WorkerResponse::decode(&response).expect("decode replacement response");
        assert_eq!(response.job_id, 43);
        let result = response.outcome.expect("replacement response");
        let WorkerResultKind::ResourceCompleted { data, .. } = result.kind else {
            panic!("unexpected replacement result kind");
        };
        assert_eq!(data.as_ref(), b"replacement response");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn worker_process_pool_prefers_idle_child_over_busy_round_robin_slot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script = temp.path().join("stalled-worker.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 5\n").expect("write worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = WorkerStdioPool::spawn(&script, 2).expect("spawn worker pool");
        let busy_slot = pool.workers[0].lock().await;
        pool.next.store(0, Ordering::SeqCst);

        let (index, idle_slot) = timeout(Duration::from_millis(50), pool.lock_worker_for_submit())
            .await
            .expect("pool should select an idle worker without waiting");

        assert_eq!(index, 1);
        drop(idle_slot);
        drop(busy_slot);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn worker_process_pool_serves_idle_child_while_peer_child_is_stalled() {
        let temp = tempfile::tempdir().expect("tempdir");
        let stalled_marker = temp.path().join("stalled-request-received");
        let script = temp.path().join("one-stalled-one-healthy-worker.py");
        let response = WorkerResponse::success(WorkerResult {
            id: 302,
            kind: WorkerResultKind::ResourceCompleted {
                resource_hash: [0x62; rns_transport::hash::HASH_SIZE],
                proof: [0x26; rns_transport::hash::HASH_SIZE],
                data: serde_bytes::ByteBuf::from(b"idle child response".to_vec()),
                metadata: None,
                request_id: None,
                is_request: false,
                is_response: false,
            },
        })
        .encode()
        .expect("encode idle child response");
        let response_frame =
            encode_worker_frame(&response, MAX_WORKER_RESPONSE_BYTES).expect("response frame");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import os
import struct
import sys
import time

header = sys.stdin.buffer.read(4)
if len(header) != 4:
    sys.exit(0)
length = struct.unpack(">I", header)[0]
sys.stdin.buffer.read(length)

marker = {stalled_marker:?}
if not os.path.exists(marker):
    open(marker, "w").close()
    time.sleep(5)
    sys.exit(0)

sys.stdout.buffer.write(bytes.fromhex({response_frame_hex:?}))
sys.stdout.buffer.flush()
"#,
                stalled_marker = stalled_marker.to_string_lossy(),
                response_frame_hex = hex::encode(response_frame),
            ),
        )
        .expect("write worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = Arc::new(WorkerStdioPool::spawn(&script, 2).expect("spawn worker pool"));
        pool.next.store(0, Ordering::SeqCst);
        let stalled_request = WorkerRequest::new(
            WorkerJob {
                id: 301,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            },
            STALLED_WORKER_TIMEOUT_MS,
        )
        .encode()
        .expect("encode stalled request");
        let stalled_pool = Arc::clone(&pool);
        let stalled_submit = tokio::spawn(async move {
            stalled_pool
                .submit_encoded(&stalled_request, Duration::from_millis(STALLED_WORKER_TIMEOUT_MS))
                .await
        });

        let marker_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !stalled_marker.exists() && tokio::time::Instant::now() < marker_deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(stalled_marker.exists(), "first worker should receive and stall on the request");
        assert_eq!(
            pool.snapshot(),
            WorkerStdioPoolSnapshot {
                worker_count: 2,
                idle_workers: 1,
                busy_workers: 1,
                request_timeouts: 0,
                child_replacements: 0,
            }
        );

        let idle_request = WorkerRequest::new(
            WorkerJob {
                id: 302,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            },
            1_000,
        )
        .encode()
        .expect("encode idle request");
        let response = timeout(
            Duration::from_secs(1),
            pool.submit_encoded(&idle_request, Duration::from_secs(1)),
        )
        .await
        .expect("idle child should serve a second request while peer child is stalled")
        .expect("idle child response");
        let response = WorkerResponse::decode(&response).expect("decode idle child response");
        assert_eq!(response.job_id, 302);
        let result = response.outcome.expect("idle child worker result");
        let WorkerResultKind::ResourceCompleted { data, .. } = result.kind else {
            panic!("unexpected idle child result kind");
        };
        assert_eq!(data.as_ref(), b"idle child response");

        let slow_err = stalled_submit
            .await
            .expect("stalled submit task should join")
            .expect_err("stalled worker should time out");
        assert!(matches!(
            slow_err,
            WorkerProcessError::RequestTimedOut { timeout_ms: STALLED_WORKER_TIMEOUT_MS }
        ));
        assert_eq!(pool.snapshot().request_timeouts, 1);
        assert_eq!(pool.snapshot().child_replacements, 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn worker_process_backend_serves_idle_child_while_peer_child_is_stalled() {
        let temp = tempfile::tempdir().expect("tempdir");
        let stalled_marker = temp.path().join("backend-stalled-request-received");
        let script = temp.path().join("one-stalled-one-healthy-backend.py");
        let response = WorkerResponse::success(WorkerResult {
            id: 402,
            kind: WorkerResultKind::ResourceCompleted {
                resource_hash: [0x64; rns_transport::hash::HASH_SIZE],
                proof: [0x46; rns_transport::hash::HASH_SIZE],
                data: serde_bytes::ByteBuf::from(b"backend idle child response".to_vec()),
                metadata: None,
                request_id: None,
                is_request: false,
                is_response: false,
            },
        })
        .encode()
        .expect("encode backend idle child response");
        let response_frame =
            encode_worker_frame(&response, MAX_WORKER_RESPONSE_BYTES).expect("response frame");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import os
import struct
import sys
import time

header = sys.stdin.buffer.read(4)
if len(header) != 4:
    sys.exit(0)
length = struct.unpack(">I", header)[0]
sys.stdin.buffer.read(length)

marker = {stalled_marker:?}
if not os.path.exists(marker):
    open(marker, "w").close()
    time.sleep(5)
    sys.exit(0)

sys.stdout.buffer.write(bytes.fromhex({response_frame_hex:?}))
sys.stdout.buffer.flush()
"#,
                stalled_marker = stalled_marker.to_string_lossy(),
                response_frame_hex = hex::encode(response_frame),
            ),
        )
        .expect("write backend worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = WorkerStdioPool::spawn(&script, 2).expect("spawn worker pool");
        pool.next.store(0, Ordering::SeqCst);
        let backend = Arc::new(WorkerStdioPoolBackend::new(pool, STALLED_WORKER_TIMEOUT_MS));
        let stalled_backend = Arc::clone(&backend);
        let stalled_submit = tokio::spawn(async move {
            stalled_backend
                .submit(WorkerJob {
                    id: 401,
                    kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
                })
                .await
        });

        let marker_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !stalled_marker.exists() && tokio::time::Instant::now() < marker_deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(stalled_marker.exists(), "first backend worker should receive and stall");

        let result = timeout(
            Duration::from_secs(1),
            backend.submit(WorkerJob {
                id: 402,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            }),
        )
        .await
        .expect("backend should use idle child while peer child is stalled")
        .expect("backend idle child result");
        let WorkerResultKind::ResourceCompleted { data, .. } = result.kind else {
            panic!("unexpected backend idle child result kind");
        };
        assert_eq!(data.as_ref(), b"backend idle child response");

        let slow_err = stalled_submit
            .await
            .expect("stalled backend task should join")
            .expect_err("stalled backend worker should time out");
        let WorkerError::BackendUnavailable { message } = slow_err else {
            panic!("unexpected stalled backend error");
        };
        assert!(message.contains("RequestTimedOut"));
        assert!(message.contains(&STALLED_WORKER_TIMEOUT_MS.to_string()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn worker_process_backend_replaces_timed_out_child_and_serves_next_request() {
        let temp = tempfile::tempdir().expect("tempdir");
        let stalled_marker = temp.path().join("backend-timeout-request-received");
        let script = temp.path().join("backend-stalled-then-healthy-worker.py");
        let response = WorkerResponse::success(WorkerResult {
            id: 502,
            kind: WorkerResultKind::ResourceCompleted {
                resource_hash: [0x65; rns_transport::hash::HASH_SIZE],
                proof: [0x56; rns_transport::hash::HASH_SIZE],
                data: serde_bytes::ByteBuf::from(b"backend replacement response".to_vec()),
                metadata: None,
                request_id: None,
                is_request: false,
                is_response: false,
            },
        })
        .encode()
        .expect("encode backend replacement response");
        let response_frame =
            encode_worker_frame(&response, MAX_WORKER_RESPONSE_BYTES).expect("response frame");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import os
import struct
import sys
import time

header = sys.stdin.buffer.read(4)
if len(header) != 4:
    sys.exit(0)
length = struct.unpack(">I", header)[0]
sys.stdin.buffer.read(length)

marker = {stalled_marker:?}
if not os.path.exists(marker):
    open(marker, "w").close()
    time.sleep(5)
    sys.exit(0)

sys.stdout.buffer.write(bytes.fromhex({response_frame_hex:?}))
sys.stdout.buffer.flush()
"#,
                stalled_marker = stalled_marker.to_string_lossy(),
                response_frame_hex = hex::encode(response_frame),
            ),
        )
        .expect("write backend replacement worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = WorkerStdioPool::spawn(&script, 1).expect("spawn worker pool");
        let backend = Arc::new(WorkerStdioPoolBackend::new(pool, STALLED_WORKER_TIMEOUT_MS));
        let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
        let runtime = crate::bootstrap::WorkerProcessRuntimeStatus {
            enabled: true,
            worker_count: 1,
            timeout_ms: STALLED_WORKER_TIMEOUT_MS,
        };
        crate::bootstrap::refresh_worker_process_status(&daemon, &runtime, Some(&backend));
        let publisher = crate::bootstrap::spawn_worker_process_status_publisher_with_interval(
            daemon.clone(),
            runtime.clone(),
            Some(backend.clone()),
            Duration::from_millis(10),
        )
        .expect("worker status publisher should start with backend");
        let status = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 5001,
                method: "daemon_status_ex".to_string(),
                params: None,
            })
            .expect("initial worker status")
            .result
            .expect("initial worker status result");
        assert_eq!(status["worker_processes"]["idle_workers"], serde_json::json!(1));
        assert_eq!(status["worker_processes"]["busy_workers"], serde_json::json!(0));
        assert_eq!(status["worker_processes"]["request_timeouts"], serde_json::json!(0));
        assert_eq!(status["worker_processes"]["child_replacements"], serde_json::json!(0));
        assert_eq!(
            backend.pool.snapshot(),
            WorkerStdioPoolSnapshot {
                worker_count: 1,
                idle_workers: 1,
                busy_workers: 0,
                request_timeouts: 0,
                child_replacements: 0,
            }
        );
        let original_child_id =
            backend.pool.workers[0].lock().await.child_id().expect("child process id");
        let err = backend
            .submit(WorkerJob {
                id: 501,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            })
            .await
            .expect_err("stalled backend worker should time out");
        let WorkerError::BackendUnavailable { message } = err else {
            panic!("unexpected backend timeout error");
        };
        assert!(message.contains("RequestTimedOut"));
        assert!(
            stalled_marker.exists(),
            "first backend worker should receive request before timeout"
        );
        assert_eq!(backend.pool.snapshot().request_timeouts, 1);
        assert_eq!(backend.pool.snapshot().child_replacements, 1);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(250);
        let status = loop {
            let status = daemon
                .handle_rpc(rns_rpc::RpcRequest {
                    id: 5002,
                    method: "daemon_status_ex".to_string(),
                    params: None,
                })
                .expect("post-timeout worker status")
                .result
                .expect("post-timeout worker status result");
            if status["worker_processes"]["request_timeouts"] == serde_json::json!(1) {
                break status;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "worker status publisher should publish timeout counters"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(status["worker_processes"]["idle_workers"], serde_json::json!(1));
        assert_eq!(status["worker_processes"]["busy_workers"], serde_json::json!(0));
        assert_eq!(status["worker_processes"]["request_timeouts"], serde_json::json!(1));
        assert_eq!(status["worker_processes"]["child_replacements"], serde_json::json!(1));

        let replacement_child_id =
            backend.pool.workers[0].lock().await.child_id().expect("child process id");
        assert_ne!(replacement_child_id, original_child_id);

        let result = backend
            .submit(WorkerJob {
                id: 502,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            })
            .await
            .expect("replacement backend worker should serve next request");
        let WorkerResultKind::ResourceCompleted { data, .. } = result.kind else {
            panic!("unexpected backend replacement result kind");
        };
        assert_eq!(data.as_ref(), b"backend replacement response");
        assert_eq!(
            backend.pool.snapshot(),
            WorkerStdioPoolSnapshot {
                worker_count: 1,
                idle_workers: 1,
                busy_workers: 0,
                request_timeouts: 1,
                child_replacements: 1,
            }
        );
        publisher.abort();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stalled_worker_process_submit_does_not_block_daemon_status_rpc() {
        let temp = tempfile::tempdir().expect("tempdir");
        let request_received = temp.path().join("stalled-worker-received");
        let script = temp.path().join("stalled-worker.py");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import struct
import sys
import time

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
open({request_received:?}, "w").close()
time.sleep(5)
"#,
                request_received = request_received.to_string_lossy(),
            ),
        )
        .expect("write stalled worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = Arc::new(WorkerStdioPool::spawn(&script, 1).expect("spawn stalled worker"));
        let request = WorkerRequest::new(
            WorkerJob {
                id: 100,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            },
            STALLED_WORKER_TIMEOUT_MS,
        )
        .encode()
        .expect("encode stalled request");
        let stalled_pool = Arc::clone(&pool);
        let stalled_submit = tokio::spawn(async move {
            stalled_pool
                .submit_encoded(&request, Duration::from_millis(STALLED_WORKER_TIMEOUT_MS))
                .await
        });

        let request_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !request_received.exists() && tokio::time::Instant::now() < request_deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(request_received.exists(), "stalled worker should receive the request");

        let daemon = rns_rpc::RpcDaemon::test_instance();
        let status = timeout(Duration::from_millis(50), async {
            daemon.handle_rpc(rns_rpc::RpcRequest {
                id: 1000,
                method: "daemon_status_ex".to_string(),
                params: None,
            })
        })
        .await
        .expect("daemon status rpc should not wait for stalled worker process")
        .expect("daemon status rpc");
        let result = status.result.expect("daemon status result");
        assert_eq!(result["worker_processes"]["enabled"].as_bool(), Some(false));

        let slow_err = stalled_submit
            .await
            .expect("stalled submit task should join")
            .expect_err("stalled worker should time out");
        assert!(matches!(
            slow_err,
            WorkerProcessError::RequestTimedOut { timeout_ms: STALLED_WORKER_TIMEOUT_MS }
        ));
    }

    struct NotifySink {
        tx: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
    }

    impl NotifySink {
        fn new(tx: std::sync::mpsc::Sender<()>) -> Self {
            Self { tx: std::sync::Mutex::new(Some(tx)) }
        }
    }

    impl rns_rpc::EventSinkBridge for NotifySink {
        fn sink_id(&self) -> &str {
            "notify-sink"
        }

        fn sink_kind(&self) -> &'static str {
            "webhook"
        }

        fn publish(&self, _envelope: &rns_rpc::RpcEventSinkEnvelope) -> Result<(), std::io::Error> {
            if let Some(tx) = self.tx.lock().expect("notify sink mutex poisoned").take() {
                let _ = tx.send(());
            }
            Ok(())
        }
    }

    struct NotifyOutboundBridge {
        tx: std::sync::Mutex<Option<std::sync::mpsc::Sender<String>>>,
    }

    impl NotifyOutboundBridge {
        fn new(tx: std::sync::mpsc::Sender<String>) -> Self {
            Self { tx: std::sync::Mutex::new(Some(tx)) }
        }
    }

    impl rns_rpc::OutboundBridge for NotifyOutboundBridge {
        fn deliver(
            &self,
            record: &rns_rpc::MessageRecord,
            _options: &rns_rpc::OutboundDeliveryOptions,
        ) -> Result<(), std::io::Error> {
            if let Some(tx) = self.tx.lock().expect("notify outbound mutex poisoned").take() {
                let _ = tx.send(record.id.clone());
            }
            Ok(())
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stalled_worker_process_submit_does_not_block_event_sink_dispatch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let request_received = temp.path().join("stalled-worker-received");
        let script = temp.path().join("stalled-worker.py");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import struct
import sys
import time

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
open({request_received:?}, "w").close()
time.sleep(5)
"#,
                request_received = request_received.to_string_lossy(),
            ),
        )
        .expect("write stalled worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = Arc::new(WorkerStdioPool::spawn(&script, 1).expect("spawn stalled worker"));
        let request = WorkerRequest::new(
            WorkerJob {
                id: 101,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            },
            STALLED_WORKER_TIMEOUT_MS,
        )
        .encode()
        .expect("encode stalled request");
        let stalled_pool = Arc::clone(&pool);
        let stalled_submit = tokio::spawn(async move {
            stalled_pool
                .submit_encoded(&request, Duration::from_millis(STALLED_WORKER_TIMEOUT_MS))
                .await
        });

        let request_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !request_received.exists() && tokio::time::Instant::now() < request_deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(request_received.exists(), "stalled worker should receive the request");

        let (sink_tx, sink_rx) = std::sync::mpsc::channel();
        let sink: Arc<dyn rns_rpc::EventSinkBridge> = Arc::new(NotifySink::new(sink_tx));
        let daemon = rns_rpc::RpcDaemon::with_store_and_bridges_and_sinks(
            rns_rpc::MessagesStore::in_memory().expect("in-memory store"),
            "event-sink-node".to_string(),
            None,
            None,
            vec![sink],
        );
        let configure = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 1001,
                method: "sdk_configure_v2".to_string(),
                params: Some(serde_json::json!({
                    "expected_revision": 0,
                    "patch": {
                        "event_sink": {
                            "enabled": true,
                            "allow_kinds": ["webhook"]
                        }
                    }
                })),
            })
            .expect("configure event sink");
        assert!(configure.error.is_none());

        daemon.emit_event(rns_rpc::RpcEvent {
            event_type: "delivery_update".to_string(),
            payload: serde_json::json!({ "message_id": "m-process-stall" }),
        });
        sink_rx
            .recv_timeout(Duration::from_millis(250))
            .expect("event sink should dispatch while worker process is stalled");

        let slow_err = stalled_submit
            .await
            .expect("stalled submit task should join")
            .expect_err("stalled worker should time out");
        assert!(matches!(
            slow_err,
            WorkerProcessError::RequestTimedOut { timeout_ms: STALLED_WORKER_TIMEOUT_MS }
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stalled_worker_process_submit_does_not_block_outbound_delivery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let request_received = temp.path().join("stalled-worker-received");
        let script = temp.path().join("stalled-worker.py");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import struct
import sys
import time

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
open({request_received:?}, "w").close()
time.sleep(5)
"#,
                request_received = request_received.to_string_lossy(),
            ),
        )
        .expect("write stalled worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = Arc::new(WorkerStdioPool::spawn(&script, 1).expect("spawn stalled worker"));
        let request = WorkerRequest::new(
            WorkerJob {
                id: 102,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            },
            STALLED_WORKER_TIMEOUT_MS,
        )
        .encode()
        .expect("encode stalled request");
        let stalled_pool = Arc::clone(&pool);
        let stalled_submit = tokio::spawn(async move {
            stalled_pool
                .submit_encoded(&request, Duration::from_millis(STALLED_WORKER_TIMEOUT_MS))
                .await
        });

        let request_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !request_received.exists() && tokio::time::Instant::now() < request_deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(request_received.exists(), "stalled worker should receive the request");

        let (delivery_tx, delivery_rx) = std::sync::mpsc::channel();
        let bridge: Arc<dyn rns_rpc::OutboundBridge> =
            Arc::new(NotifyOutboundBridge::new(delivery_tx));
        let daemon = rns_rpc::RpcDaemon::with_store_and_bridges(
            rns_rpc::MessagesStore::in_memory().expect("in-memory store"),
            "outbound-node".to_string(),
            Some(bridge),
            None,
        );
        let send = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 1002,
                method: "send_message_v2".to_string(),
                params: Some(serde_json::json!({
                    "id": "process-stall-outbound",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hello"
                })),
            })
            .expect("send outbound");
        assert!(send.error.is_none());
        let delivered = delivery_rx
            .recv_timeout(Duration::from_millis(250))
            .expect("outbound delivery should start while worker process is stalled");
        assert_eq!(delivered, "process-stall-outbound");

        let slow_err = stalled_submit
            .await
            .expect("stalled submit task should join")
            .expect_err("stalled worker should time out");
        assert!(matches!(
            slow_err,
            WorkerProcessError::RequestTimedOut { timeout_ms: STALLED_WORKER_TIMEOUT_MS }
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn worker_process_restart_does_not_corrupt_daemon_message_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let request_received = temp.path().join("stalled-worker-received");
        let script = temp.path().join("stalled-then-healthy-worker.py");
        let response = WorkerResponse::success(WorkerResult {
            id: 202,
            kind: WorkerResultKind::ResourceCompleted {
                resource_hash: [0x42; rns_transport::hash::HASH_SIZE],
                proof: [0x24; rns_transport::hash::HASH_SIZE],
                data: serde_bytes::ByteBuf::from(b"replacement alive".to_vec()),
                metadata: None,
                request_id: None,
                is_request: false,
                is_response: false,
            },
        })
        .encode()
        .expect("encode replacement response");
        let response_frame =
            encode_worker_frame(&response, MAX_WORKER_RESPONSE_BYTES).expect("response frame");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import os
import struct
import sys
import time

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)

marker = {request_received:?}
if not os.path.exists(marker):
    open(marker, "w").close()
    time.sleep(5)
    sys.exit(0)

sys.stdout.buffer.write(bytes.fromhex({response_frame_hex:?}))
sys.stdout.buffer.flush()
"#,
                request_received = request_received.to_string_lossy(),
                response_frame_hex = hex::encode(response_frame),
            ),
        )
        .expect("write worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let pool = Arc::new(WorkerStdioPool::spawn(&script, 1).expect("spawn stalled worker"));
        let original_child_id = pool.workers[0].lock().await.child_id().expect("child process id");
        let request = WorkerRequest::new(
            WorkerJob {
                id: 201,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            },
            STALLED_WORKER_TIMEOUT_MS,
        )
        .encode()
        .expect("encode stalled request");
        let stalled_pool = Arc::clone(&pool);
        let stalled_submit = tokio::spawn(async move {
            stalled_pool
                .submit_encoded(&request, Duration::from_millis(STALLED_WORKER_TIMEOUT_MS))
                .await
        });

        let request_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !request_received.exists() && tokio::time::Instant::now() < request_deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(request_received.exists(), "stalled worker should receive the request");

        let (delivery_tx, delivery_rx) = std::sync::mpsc::channel();
        let bridge: Arc<dyn rns_rpc::OutboundBridge> =
            Arc::new(NotifyOutboundBridge::new(delivery_tx));
        let daemon = rns_rpc::RpcDaemon::with_store_and_bridges(
            rns_rpc::MessagesStore::in_memory().expect("in-memory store"),
            "restart-state-node".to_string(),
            Some(bridge),
            None,
        );
        let send = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 2001,
                method: "send_message_v2".to_string(),
                params: Some(serde_json::json!({
                    "id": "worker-restart-state",
                    "source": "src",
                    "destination": "dst",
                    "title": "state",
                    "content": "survives restart"
                })),
            })
            .expect("send outbound while worker is stalled");
        assert!(send.error.is_none());
        let delivered = delivery_rx
            .recv_timeout(Duration::from_millis(250))
            .expect("outbound delivery should start while worker process is stalled");
        assert_eq!(delivered, "worker-restart-state");

        let receipt = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 2002,
                method: "record_receipt".to_string(),
                params: Some(serde_json::json!({
                    "message_id": "worker-restart-state",
                    "status": "delivered"
                })),
            })
            .expect("record receipt while worker is stalled");
        assert!(receipt.error.is_none());
        assert_eq!(
            receipt.result.expect("receipt result")["status"],
            serde_json::json!("delivered")
        );
        let configure = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 2004,
                method: "sdk_configure_v2".to_string(),
                params: Some(serde_json::json!({
                    "expected_revision": 0,
                    "patch": {
                        "overflow_policy": "drop_oldest"
                    }
                })),
            })
            .expect("configure sdk while worker is stalled");
        assert!(configure.error.is_none());
        assert_eq!(configure.result.expect("configure result")["revision"], serde_json::json!(1));
        let announce = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 2007,
                method: "announce_received".to_string(),
                params: Some(serde_json::json!({
                    "peer": "worker-restart-route",
                    "timestamp": 1_700_000_222i64,
                    "name": "Route Peer",
                    "name_source": "announce",
                    "capabilities": ["lxmf.delivery", "propagation"],
                    "stamp_cost": 17,
                    "stamp_cost_flexibility": 3
                })),
            })
            .expect("record route announce while worker is stalled");
        assert!(announce.error.is_none());

        let slow_err = stalled_submit
            .await
            .expect("stalled submit task should join")
            .expect_err("stalled worker should time out");
        assert!(matches!(
            slow_err,
            WorkerProcessError::RequestTimedOut { timeout_ms: STALLED_WORKER_TIMEOUT_MS }
        ));
        let replacement_child_id =
            pool.workers[0].lock().await.child_id().expect("child process id");
        assert_ne!(replacement_child_id, original_child_id);

        let next_request = WorkerRequest::new(
            WorkerJob {
                id: 202,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"not-used".to_vec() },
            },
            1_000,
        )
        .encode()
        .expect("encode replacement request");
        let response = pool
            .submit_encoded(&next_request, Duration::from_secs(1))
            .await
            .expect("replacement worker should serve next request");
        let response = WorkerResponse::decode(&response).expect("decode replacement response");
        assert_eq!(response.job_id, 202);

        let snapshot = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 2005,
                method: "sdk_snapshot_v2".to_string(),
                params: Some(serde_json::json!({ "include_counts": true })),
            })
            .expect("sdk snapshot after worker restart")
            .result
            .expect("sdk snapshot result");
        assert_eq!(snapshot["state"], serde_json::json!("running"));
        assert_eq!(snapshot["config_revision"], serde_json::json!(1));
        let configure_after_restart = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 2006,
                method: "sdk_configure_v2".to_string(),
                params: Some(serde_json::json!({
                    "expected_revision": 1,
                    "patch": {
                        "overflow_policy": "reject"
                    }
                })),
            })
            .expect("configure sdk after worker restart");
        assert!(configure_after_restart.error.is_none());
        assert_eq!(
            configure_after_restart.result.expect("post-restart configure result")["revision"],
            serde_json::json!(2)
        );

        let announces = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 2008,
                method: "list_announces".to_string(),
                params: None,
            })
            .expect("list announces after worker restart")
            .result
            .expect("list announces result");
        let announces = announces["announces"].as_array().expect("announces array");
        let route = announces
            .iter()
            .find(|announce| announce["peer"] == serde_json::json!("worker-restart-route"))
            .expect("announce route state should survive worker restart");
        assert_eq!(route["timestamp"], serde_json::json!(1_700_000_222i64));
        assert_eq!(route["name"], serde_json::json!("Route Peer"));
        assert_eq!(route["stamp_cost"], serde_json::json!(17));

        let messages = daemon
            .handle_rpc(rns_rpc::RpcRequest {
                id: 2003,
                method: "list_messages".to_string(),
                params: None,
            })
            .expect("list messages after worker restart")
            .result
            .expect("list messages result");
        let messages = messages["messages"].as_array().expect("messages array");
        let record = messages
            .iter()
            .find(|message| message["id"] == serde_json::json!("worker-restart-state"))
            .expect("message should survive worker restart");
        assert_eq!(record["content"], serde_json::json!("survives restart"));
        assert_eq!(record["receipt_status"], serde_json::json!("delivered"));
    }
}
