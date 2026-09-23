use super::PipeInterface;
use crate::iface::{InterfaceManager, InterfaceSharedConfig, TxMessage, TxMessageType};
use crate::packet::{Packet, PacketDataBuffer};
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
