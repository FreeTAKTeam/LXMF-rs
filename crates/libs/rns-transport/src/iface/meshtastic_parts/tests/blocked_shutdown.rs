use super::*;
use crate::iface::{InterfaceSharedConfig, TxMessageType};
use crate::packet::{Packet, PacketDataBuffer};
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::task::Poll;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

fn packet() -> Packet {
    Packet {
        destination: AddressHash::new([0xd1; 16]),
        data: PacketDataBuffer::new_from_slice(b"native-mesh-handoff"),
        ..Default::default()
    }
}

async fn poll_pending<F: Future>(mut worker: Pin<&mut F>) {
    poll_fn(|context| {
        assert!(worker.as_mut().poll(context).is_pending(), "native worker is held at a handoff");
        Poll::Ready(())
    })
    .await;
}

async fn join(mut worker: tokio::task::JoinHandle<()>) {
    match timeout(Duration::from_secs(2), &mut worker).await {
        Ok(result) => result.expect("native Meshtastic worker joined"),
        Err(error) => {
            worker.abort();
            assert!(worker.await.expect_err("abort stalled native worker").is_cancelled());
            panic!(
                "native Meshtastic shutdown must complete while its queue remains full: {error}"
            );
        }
    }
}

async fn blocked_shutdown(full_output: bool) {
    let config = MeshtasticInterfaceConfig {
        max_payload_bytes: 64,
        send_delay: Duration::from_millis(5),
        ..Default::default()
    };
    let interface = MeshtasticInterface::new("owned queue-pressure fixture", config.clone());
    let handle = interface.handle();
    let status = interface.runtime_status_handle();
    let output_sender = interface.outbound_tx.clone();
    let mut manager = InterfaceManager::new(1);
    let mut context = manager.new_context(interface);
    context.cancel = CancellationToken::new();
    let global_stop = context.cancel.clone();
    let iface_stop = context.channel.stop.clone();
    let address = context.channel.address;
    assert_eq!(context.channel.ifac_default_size_bytes, 16);
    let shared = InterfaceSharedConfig {
        network_name: Some("mesh-pressure-network".into()),
        passphrase: Some("mesh-pressure-credential".into()),
        ..Default::default()
    };
    assert_eq!(shared.ifac_size, None);
    assert!(manager.try_set_shared_config(address, shared).expect("prepared native IFAC"));
    let state = context.channel.ifac_state.clone();
    let violations = context.channel.ifac_violations.clone();
    let receiver = manager.receiver();
    let sentinel = RxMessage { address, packet: Packet::default(), source: IfaceSource::None };
    let mut held_output = Vec::new();
    if full_output {
        for index in 0..MESHTASTIC_CHANNEL_CAPACITY {
            let frame = MeshtasticTransmitFrame {
                destination: MeshtasticDestination::Node(index as u32),
                payload: vec![0xee, index as u8],
                hop_limit: 7,
                want_ack: false,
                want_response: false,
                channel_index: 0,
            };
            output_sender.try_send(frame.clone()).expect("prefill actual native output queue");
            held_output.push(frame);
        }
        assert_eq!(output_sender.capacity(), 0);
    } else {
        context.channel.rx_channel.send(sentinel.clone()).await.expect("prefill native RX queue");
        assert_eq!(context.channel.rx_channel.capacity(), 0);
    }
    let mut worker = Box::pin(MeshtasticInterface::spawn(context));
    // No input yet: consume the interval's initial tick without select-order assumptions.
    poll_pending(worker.as_mut()).await;
    assert_eq!(status.snapshot().queued_transmissions, 0);
    let input = packet();
    let bytes = encode_packet_ifac(&state, &input).expect("native authenticated packet");
    assert_eq!(bytes.len(), input.to_bytes().expect("plain packet").len() + 16);
    assert_eq!(decode_packet_ifac(&state, &bytes).expect("valid exact IFAC").data, input.data);
    let mut tunnel = MeshtasticTunnel::new(config.clone());
    tunnel.queue_outgoing_packet(&bytes).expect("native tunnel preparation");
    let frame = tunnel.next_transmit().expect("one native tunnel frame");
    assert!(tunnel.next_transmit().is_none(), "one complete nonempty test frame");
    let mut roundtrip = MeshtasticTunnel::new(config.clone());
    assert_eq!(
        roundtrip
            .process_received(MeshtasticReceivedFrame::new(41, &frame.payload))
            .expect("valid tunnel reassembly"),
        Some(bytes)
    );
    if full_output {
        assert_eq!(
            manager
                .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: input })
                .await
                .sent_ifaces,
            1
        );
        poll_pending(worker.as_mut()).await;
        assert_eq!(status.snapshot().queued_transmissions, 1, "native packet queued before tick");
        assert_eq!(status.snapshot().chunks_tx, 0);
        sleep(config.send_delay).await;
        poll_pending(worker.as_mut()).await;
        assert_eq!(output_sender.capacity(), 0);
        assert_eq!(handle.inbound_tx.capacity(), MESHTASTIC_CHANNEL_CAPACITY);
        iface_stop.cancel();
        assert!(!global_stop.is_cancelled());
    } else {
        handle
            .inject_received(MeshtasticReceivedFrame::new(41, &frame.payload))
            .await
            .expect("one actual native receive input");
        assert_eq!(handle.inbound_tx.capacity(), MESHTASTIC_CHANNEL_CAPACITY - 1);
        poll_pending(worker.as_mut()).await;
        assert_eq!(handle.inbound_tx.capacity(), MESHTASTIC_CHANNEL_CAPACITY, "input consumed");
        assert_eq!(receiver.lock().await.len(), 1);
        assert_eq!(status.snapshot().decode_errors, 0);
        assert_eq!(violations.load(std::sync::atomic::Ordering::Relaxed), 0);
        global_stop.cancel();
        assert!(!iface_stop.is_cancelled());
    }
    join(tokio::spawn(worker)).await;
    assert!(iface_stop.is_cancelled(), "worker publishes native stop state");
    assert!(handle.inbound_tx.is_closed(), "joined worker releases native input receiver");
    if full_output {
        assert_eq!(output_sender.capacity(), 0, "output remains full through join");
        let mut output = handle.outbound_rx.lock().await;
        for expected in held_output {
            assert_eq!(output.try_recv().expect("exact held output"), expected);
        }
        assert!(output.try_recv().is_err(), "cancelled output is never enqueued");
        assert!(receiver.lock().await.try_recv().is_err());
        assert_eq!(status.snapshot().chunks_tx, 0, "cancelled handoff is not published as output");
    } else {
        assert_eq!(receiver.lock().await.len(), 1, "RX remains full through join");
        assert_eq!(receiver.lock().await.try_recv().expect("exact held RX"), sentinel);
        assert!(receiver.lock().await.try_recv().is_err(), "cancelled input is not admitted");
    }
    assert!(manager.stop_interface(address));
    assert!(manager.interface_hashes().is_empty());
}

#[tokio::test]
async fn meshtastic_shutdown_joins_full_native_receive_queue() {
    blocked_shutdown(false).await;
}

#[tokio::test]
async fn meshtastic_shutdown_joins_full_native_output_queue() {
    blocked_shutdown(true).await;
}
