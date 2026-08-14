use std::env;
#[cfg(target_os = "linux")]
use std::fs;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener as StdTcpListener};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rns_transport::buffer::OutputBuffer;
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::iface::{InterfaceManager, TxMessage, TxMessageType};
use rns_transport::packet::{Packet, PacketDataBuffer};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

#[derive(Clone, Copy)]
enum Activity {
    Idle,
    Small,
    Maximum,
}

impl Activity {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "idle" => Ok(Self::Idle),
            "small" => Ok(Self::Small),
            "maximum" => Ok(Self::Maximum),
            _ => Err(format!("unknown activity '{value}'; expected idle, small, or maximum")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Small => "small",
            Self::Maximum => "maximum",
        }
    }
}

struct Args {
    connections: usize,
    activity: Activity,
    mtu: usize,
    settle: Duration,
    broadcasts: usize,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut args = env::args().skip(1);
        let mut parsed = Self {
            connections: 100,
            activity: Activity::Idle,
            mtu: TcpClient::DEFAULT_MTU,
            settle: Duration::from_millis(500),
            broadcasts: 0,
        };
        while let Some(flag) = args.next() {
            let value = args.next().ok_or_else(|| format!("missing value for {flag}"))?;
            match flag.as_str() {
                "--connections" => {
                    parsed.connections = value.parse().map_err(|_| "invalid connection count")?;
                }
                "--activity" => parsed.activity = Activity::parse(&value)?,
                "--mtu" => parsed.mtu = value.parse().map_err(|_| "invalid MTU")?,
                "--settle-ms" => {
                    let millis = value.parse().map_err(|_| "invalid settle time")?;
                    parsed.settle = Duration::from_millis(millis);
                }
                "--broadcasts" => {
                    parsed.broadcasts = value.parse().map_err(|_| "invalid broadcast count")?;
                }
                _ => return Err(format!("unknown option {flag}")),
            }
        }
        if parsed.connections == 0 {
            return Err("connections must be greater than zero".to_string());
        }
        if parsed.mtu < 256 {
            return Err("MTU must be at least 256".to_string());
        }
        Ok(parsed)
    }
}

#[derive(Clone, Copy, Default)]
struct ProcessMemory {
    rss_kib: Option<u64>,
    peak_rss_kib: Option<u64>,
    virtual_kib: Option<u64>,
}

impl ProcessMemory {
    fn sample() -> Self {
        #[cfg(target_os = "linux")]
        {
            let Ok(status) = fs::read_to_string("/proc/self/status") else {
                return Self::default();
            };
            Self {
                rss_kib: status_kib(&status, "VmRSS:"),
                peak_rss_kib: status_kib(&status, "VmHWM:"),
                virtual_kib: status_kib(&status, "VmSize:"),
            }
        }
        #[cfg(not(target_os = "linux"))]
        Self::default()
    }

    fn json(self) -> Value {
        json!({
            "rss_kib": self.rss_kib,
            "peak_rss_kib": self.peak_rss_kib,
            "virtual_kib": self.virtual_kib,
        })
    }
}

#[cfg(target_os = "linux")]
fn status_kib(status: &str, key: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key)?.split_whitespace().next()?.parse::<u64>().ok())
}

#[derive(Clone, Copy, Default)]
struct CpuTicks {
    user: u64,
    system: u64,
}

impl CpuTicks {
    fn sample() -> Self {
        #[cfg(target_os = "linux")]
        {
            let Ok(stat) = fs::read_to_string("/proc/self/stat") else {
                return Self::default();
            };
            let Some(after_name) = stat.rsplit_once(") ").map(|(_, fields)| fields) else {
                return Self::default();
            };
            let fields = after_name.split_whitespace().collect::<Vec<_>>();
            Self {
                user: fields.get(11).and_then(|value| value.parse().ok()).unwrap_or(0),
                system: fields.get(12).and_then(|value| value.parse().ok()).unwrap_or(0),
            }
        }
        #[cfg(not(target_os = "linux"))]
        Self::default()
    }

    fn elapsed(self, start: Self) -> u64 {
        self.user
            .saturating_sub(start.user)
            .saturating_add(self.system.saturating_sub(start.system))
    }
}

fn reserve_loopback_addr() -> io::Result<SocketAddrV4> {
    let listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let addr = match listener.local_addr()? {
        std::net::SocketAddr::V4(addr) => addr,
        std::net::SocketAddr::V6(_) => unreachable!("IPv4 loopback bind returned IPv6"),
    };
    drop(listener);
    Ok(addr)
}

fn packet_frame(payload_len: usize, mtu: usize) -> Result<Vec<u8>, String> {
    let packet = Packet {
        data: PacketDataBuffer::new_from_slice(&vec![0x55; payload_len]),
        ..Default::default()
    };
    let raw = packet.to_bytes().map_err(|err| format!("packet serialization failed: {err:?}"))?;
    if raw.len() > mtu {
        return Err(format!("serialized packet {} exceeds MTU {mtu}", raw.len()));
    }
    let mut wire = vec![0_u8; raw.len().saturating_mul(2).saturating_add(2)];
    let mut output = OutputBuffer::new(&mut wire);
    let used =
        Hdlc::encode(&raw, &mut output).map_err(|err| format!("HDLC encode failed: {err:?}"))?;
    wire.truncate(used);
    Ok(wire)
}

async fn wait_for_count<F>(mut observed: F, expected: usize, label: &str) -> Result<(), String>
where
    F: FnMut() -> usize,
{
    let deadline = Instant::now() + Duration::from_secs(30);
    while observed() < expected {
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for {label}: expected {expected}, got {}",
                observed()
            ));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse().map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
    let addr = reserve_loopback_addr()?;
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(4096)));
    let receiver = manager.lock().await.receiver();
    let received_packets = Arc::new(AtomicUsize::new(0));
    let receive_task = {
        let received_packets = received_packets.clone();
        tokio::spawn(async move {
            loop {
                let message = receiver.lock().await.recv().await;
                if message.is_none() {
                    break;
                }
                received_packets.fetch_add(1, Ordering::Relaxed);
            }
        })
    };

    let server = TcpServer::new(addr.to_string(), manager.clone()).with_client_mtu(args.mtu);
    let server_status = server.runtime_status_handle();
    let server_iface = manager.lock().await.spawn(server, TcpServer::spawn);
    wait_for_count(
        || usize::from(server_status.to_json()["listener_state"].as_str() == Some("listening")),
        1,
        "TCP listener startup",
    )
    .await
    .map_err(io::Error::other)?;
    let baseline_memory = ProcessMemory::sample();
    let baseline_tasks = tokio::runtime::Handle::current().metrics().num_alive_tasks();

    let mut clients = Vec::with_capacity(args.connections);
    for _ in 0..args.connections {
        clients.push(TcpStream::connect(addr).await?);
    }
    wait_for_count(
        || {
            server_status.to_json()["accepted_connections"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(0)
        },
        args.connections,
        "accepted connections",
    )
    .await
    .map_err(io::Error::other)?;
    tokio::time::sleep(args.settle).await;
    let connected_memory = ProcessMemory::sample();
    let connected_tasks = tokio::runtime::Handle::current().metrics().num_alive_tasks();

    let cpu_start = CpuTicks::sample();
    let activity_started = Instant::now();
    let mut application_bytes = 0_usize;
    match args.activity {
        Activity::Idle => {}
        Activity::Small => {
            let frame = packet_frame(64, args.mtu).map_err(io::Error::other)?;
            application_bytes = frame.len().saturating_mul(clients.len());
            for client in &mut clients {
                client.write_all(&frame).await?;
            }
            wait_for_count(
                || received_packets.load(Ordering::Relaxed),
                clients.len(),
                "small inbound packets",
            )
            .await
            .map_err(io::Error::other)?;
        }
        Activity::Maximum => {
            let base_len = Packet::default().to_bytes()?.len();
            let payload_len = args.mtu.saturating_sub(base_len);
            let frame = packet_frame(payload_len, args.mtu).map_err(io::Error::other)?;
            application_bytes = frame.len().saturating_mul(clients.len());
            for client in &mut clients {
                client.write_all(&frame).await?;
            }
            wait_for_count(
                || received_packets.load(Ordering::Relaxed),
                clients.len(),
                "maximum inbound packets",
            )
            .await
            .map_err(io::Error::other)?;
        }
    }

    let mut broadcast_matched = 0_usize;
    let mut broadcast_sent = 0_usize;
    let mut broadcast_failed = 0_usize;
    for _ in 0..args.broadcasts {
        let packet =
            Packet { data: PacketDataBuffer::new_from_slice(&[0x42; 64]), ..Default::default() };
        let trace = manager
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet })
            .await;
        broadcast_matched = broadcast_matched.saturating_add(trace.matched_ifaces);
        broadcast_sent = broadcast_sent.saturating_add(trace.sent_ifaces);
        broadcast_failed = broadcast_failed.saturating_add(trace.failed_ifaces);
    }

    let activity_elapsed = activity_started.elapsed();
    let cpu_ticks = CpuTicks::sample().elapsed(cpu_start);
    tokio::time::sleep(args.settle).await;
    let activity_memory = ProcessMemory::sample();
    let activity_tasks = tokio::runtime::Handle::current().metrics().num_alive_tasks();
    let elapsed_seconds = activity_elapsed.as_secs_f64();
    let packet_rate = if elapsed_seconds > 0.0 {
        received_packets.load(Ordering::Relaxed) as f64 / elapsed_seconds
    } else {
        0.0
    };
    let byte_rate =
        if elapsed_seconds > 0.0 { application_bytes as f64 / elapsed_seconds } else { 0.0 };

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "activity": args.activity.as_str(),
            "connections": args.connections,
            "mtu": args.mtu,
            "server_iface": server_iface.to_string(),
            "baseline": {
                "memory": baseline_memory.json(),
                "tokio_tasks": baseline_tasks,
            },
            "connected": {
                "memory": connected_memory.json(),
                "tokio_tasks": connected_tasks,
            },
            "after_activity": {
                "memory": activity_memory.json(),
                "tokio_tasks": activity_tasks,
                "received_packets": received_packets.load(Ordering::Relaxed),
                "wire_bytes": application_bytes,
                "elapsed_ms": activity_elapsed.as_millis(),
                "cpu_ticks": cpu_ticks,
                "packets_per_second": packet_rate,
                "wire_bytes_per_second": byte_rate,
            },
            "broadcasts": {
                "attempts": args.broadcasts,
                "matched_ifaces": broadcast_matched,
                "sent_ifaces": broadcast_sent,
                "failed_ifaces": broadcast_failed,
            },
        }))?
    );

    manager.lock().await.stop_interface(server_iface);
    drop(clients);
    receive_task.abort();
    Ok(())
}
