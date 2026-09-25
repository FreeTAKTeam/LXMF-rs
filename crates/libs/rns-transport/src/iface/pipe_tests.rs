use super::PipeInterface;
use std::time::Duration;

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
