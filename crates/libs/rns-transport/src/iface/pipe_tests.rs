use super::PipeInterface;
use crate::buffer::OutputBuffer;
use crate::iface::{
    encode_packet_ifac, hdlc::Hdlc, IfacState, InterfaceManager, InterfaceSharedConfig, TxMessage,
    TxMessageType,
};
use crate::packet::{Packet, PacketDataBuffer};
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

#[test]
fn pipe_command_parser_matches_python_shlex_baseline() {
    let argv = PipeInterface::parse_command("prog --flag 'two words'").expect("parse");
    assert_eq!(argv, vec!["prog", "--flag", "two words"]);
    assert!(PipeInterface::parse_command("'unterminated").is_err());
    assert!(PipeInterface::parse_command("").is_err());
}

#[test]
fn pipe_builder_exposes_defaults_and_overrides() {
    let adapter =
        PipeInterface::new("cat").with_respawn_delay(Duration::from_millis(250)).with_mtu(512);
    assert_eq!(adapter.command(), "cat");
    assert_eq!(adapter.mtu_value(), 512);
    let status = adapter.runtime_status_json();
    assert_eq!(status["command"].as_str(), Some("cat"));
    assert_eq!(status["process_state"].as_str(), Some("configured"));
    assert_eq!(status["pipe_is_open"].as_bool(), Some(false));
    assert_eq!(status["respawn_attempts"].as_u64(), Some(0));
    assert!(status["last_error"].is_null());
}

#[test]
fn pipe_runtime_status_handle_records_respawn_errors() {
    let adapter = PipeInterface::new("cat");
    let status = adapter.runtime_status_handle();

    status.record_error_for_test("respawning", "spawn cat failed");

    let json = status.to_json();
    assert_eq!(json["command"].as_str(), Some("cat"));
    assert_eq!(json["process_state"].as_str(), Some("respawning"));
    assert_eq!(json["pipe_is_open"].as_bool(), Some(false));
    assert_eq!(json["respawn_attempts"].as_u64(), Some(1));
    assert_eq!(json["last_error"].as_str(), Some("spawn cat failed"));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn pipe_child_exit_respawns_packet_io_and_interface_cancellation_reaps_child() {
    use crate::iface::{IfaceRole, InterfaceManager, TxMessage, TxMessageType};
    use crate::packet::{Packet, PacketDataBuffer};
    use std::fs;
    use std::time::Instant;

    let temp = tempfile::tempdir_in("/dev/shm")
        .expect("create temporary PipeInterface directory in shared memory");
    let marker = temp.path().join("first-child-started");
    let child_pid = temp.path().join("second-child.pid");
    let script = temp.path().join("pipe-child.sh");
    fs::write(
        &script,
        format!(
            "if [ ! -e '{}' ]; then : > '{}'; exit 0; fi\necho $$ > '{}'\nexec cat\n",
            marker.display(),
            marker.display(),
            child_pid.display()
        ),
    )
    .expect("write deterministic child script");

    let command = format!("sh {}", script.display());
    let adapter = PipeInterface::new(command).with_respawn_delay(Duration::from_millis(10));
    let status = adapter.runtime_status_handle();
    let mut manager = InterfaceManager::new(8);
    let context = manager.new_context_with_role(adapter, IfaceRole::Unicast);
    let iface_address = context.channel.address;
    let stop = context.channel.stop.clone();
    let receiver = manager.receiver();
    let worker = tokio::spawn(PipeInterface::spawn(context));

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let snapshot = status.to_json();
        if snapshot["respawn_attempts"].as_u64().unwrap_or_default() >= 1
            && snapshot["process_state"] == "running"
        {
            break;
        }
        assert!(Instant::now() < deadline, "PipeInterface did not respawn: {snapshot}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let packet = Packet {
        data: PacketDataBuffer::new_from_slice(b"pipe-respawn-packet-roundtrip"),
        ..Packet::default()
    };
    manager
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: packet.clone() })
        .await;
    let received = tokio::time::timeout(Duration::from_secs(2), async {
        receiver.lock().await.recv().await.expect("Pipe receive channel")
    })
    .await
    .expect("restarted Pipe child packet echo deadline");
    assert_eq!(received.packet, packet);
    assert_eq!(received.address, iface_address);

    stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .expect("PipeInterface worker exits after cancellation")
        .expect("PipeInterface worker task joins");

    let child_pid = fs::read_to_string(child_pid)
        .expect("read second child process ID")
        .trim()
        .parse::<u32>()
        .expect("parse second child process ID");
    let child_still_running = std::process::Command::new("sh")
        .args(["-c", &format!("kill -0 {child_pid} 2>/dev/null")])
        .status()
        .expect("check child process liveness")
        .success();
    assert!(!child_still_running, "PipeInterface child {child_pid} survived worker shutdown");

    let snapshot = status.to_json();
    assert_eq!(snapshot["process_state"], "stopped");
    assert_eq!(snapshot["pipe_is_open"], false);
    assert_eq!(snapshot["respawn_attempts"], 1);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn pipe_child_is_terminated_when_worker_task_is_aborted() {
    use crate::iface::{IfaceRole, InterfaceManager};
    use std::fs;
    use std::time::Instant;

    let temp = tempfile::tempdir_in("/dev/shm")
        .expect("create temporary PipeInterface directory in shared memory");
    let child_pid_path = temp.path().join("child.pid");
    let script = temp.path().join("pipe-child.sh");
    fs::write(&script, format!("echo $$ > '{}'; exec sleep 60\n", child_pid_path.display()))
        .expect("write pipe child script");

    let adapter = PipeInterface::new(format!("sh {}", script.display()));
    let mut manager = InterfaceManager::new(8);
    let context = manager.new_context_with_role(adapter, IfaceRole::Unicast);
    let worker = tokio::spawn(PipeInterface::spawn(context));

    let deadline = Instant::now() + Duration::from_secs(3);
    while !child_pid_path.exists() {
        assert!(Instant::now() < deadline, "PipeInterface child did not start");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let child_pid = fs::read_to_string(&child_pid_path)
        .expect("read child process ID")
        .trim()
        .parse::<u32>()
        .expect("parse child process ID");

    worker.abort();
    let _ = worker.await;

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let still_running = std::process::Command::new("sh")
            .args(["-c", &format!("kill -0 {child_pid} 2>/dev/null")])
            .status()
            .expect("check child process liveness")
            .success();
        if !still_running {
            break;
        }
        if Instant::now() >= deadline {
            let _ =
                std::process::Command::new("kill").arg("-KILL").arg(child_pid.to_string()).status();
            panic!("PipeInterface child {child_pid} survived worker task abort");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn pipe_worker_roundtrips_authenticated_packet_with_reference_default_tag_size() {
    let mut manager = InterfaceManager::new(8);
    let pipe = PipeInterface::new("cat").with_respawn_delay(Duration::from_millis(10));
    let status = pipe.runtime_status_handle();
    let context = manager.new_context(pipe);
    let address = *context.channel.address();
    let ifac_violations = context.channel.ifac_violations.clone();

    assert_eq!(context.channel.ifac_default_size_bytes, 8);
    assert!(manager.set_shared_config(
        address,
        InterfaceSharedConfig {
            network_name: Some("pipe-ifac-test".to_string()),
            passphrase: Some("non-secret-pipe-test-credential".to_string()),
            ..InterfaceSharedConfig::default()
        },
    ));

    let receiver = manager.receiver();
    tokio::spawn(PipeInterface::spawn(context));

    let packet = Packet {
        destination: crate::hash::AddressHash::new_from_slice(&[0x42; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated pipe frame"),
        ..Packet::default()
    };
    let trace = manager
        .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: packet.clone() })
        .await;
    assert_eq!(trace.sent_ifaces, 1);

    let received = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = {
                let mut receiver = receiver.lock().await;
                receiver.try_recv()
            };
            match result {
                Ok(message) => return message,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    panic!("pipe receive channel disconnected before authenticated echo");
                }
            }
        }
    })
    .await
    .expect("pipe worker should admit its authenticated HDLC echo");

    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, packet.destination);
    assert_eq!(received.packet.data.as_slice(), b"authenticated pipe frame");
    assert_eq!(received.packet.ifac.map(|ifac| ifac.length), Some(0));
    assert_eq!(ifac_violations.load(std::sync::atomic::Ordering::Relaxed), 0);

    assert!(manager.stop_interface(address));
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if status.to_json()["process_state"].as_str() == Some("stopped") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("stopping the interface should terminate its child process");
}

#[tokio::test]
async fn pipe_stream_rejects_wrong_ifac_key_before_admission() {
    let mut manager = InterfaceManager::new(8);
    let context = manager.new_context(PipeInterface::new("cat"));
    let address = *context.channel.address();
    let ifac_state = context.channel.ifac_state.clone();
    let ifac_violations = context.channel.ifac_violations.clone();
    assert!(manager.set_shared_config(
        address,
        InterfaceSharedConfig {
            network_name: Some("pipe-ifac-ingress".to_string()),
            passphrase: Some("pipe-ifac-ingress-secret".to_string()),
            ..InterfaceSharedConfig::default()
        },
    ));

    let wrong_state: IfacState = Arc::new(std::sync::RwLock::new(
        InterfaceSharedConfig {
            network_name: Some("pipe-ifac-ingress".to_string()),
            passphrase: Some("wrong-pipe-ifac-ingress-secret".to_string()),
            ..InterfaceSharedConfig::default()
        }
        .ifac_context_with_default_size(8)
        .expect("derive wrong-key IFAC context"),
    ));
    let packet = Packet {
        destination: crate::hash::AddressHash::new_from_slice(&[0x43; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong-key Pipe frame"),
        ..Packet::default()
    };
    let payload = encode_packet_ifac(&wrong_state, &packet).expect("encode wrong-key packet");
    let mut frame = vec![0_u8; payload.len().saturating_mul(2) + 8];
    let mut output = OutputBuffer::new(&mut frame[..]);
    Hdlc::encode(&payload, &mut output).expect("HDLC encode wrong-key packet");

    let (stream, mut peer) = tokio::io::duplex(4096);
    let (rx_channel, mut rx_receiver) = mpsc::channel(8);
    let (_tx_sender, tx_receiver) = mpsc::channel(8);
    let runtime_status = Arc::new(std::sync::Mutex::new(super::PipeRuntimeStatus::new("test")));
    let cancel = CancellationToken::new();
    let worker = tokio::spawn(super::run_pipe_stream(
        stream,
        tokio::io::sink(),
        address,
        PipeInterface::DEFAULT_MTU,
        cancel.clone(),
        CancellationToken::new(),
        rx_channel,
        Arc::new(tokio::sync::Mutex::new(tx_receiver)),
        runtime_status,
        ifac_state,
        ifac_violations.clone(),
    ));

    peer.write_all(output.as_slice()).await.expect("write wrong-key Pipe frame");
    timeout(Duration::from_secs(1), async {
        while ifac_violations.load(Ordering::Relaxed) == 0 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("wrong-key Pipe frame should be counted as an IFAC violation");
    assert!(
        rx_receiver.try_recv().is_err(),
        "wrong-key Pipe packet must not reach transport admission"
    );
    cancel.cancel();
    timeout(Duration::from_secs(1), worker)
        .await
        .expect("Pipe stream worker should stop")
        .expect("Pipe stream worker task");
}
