use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{duplex, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio_util::sync::CancellationToken;

use crate::buffer::OutputBuffer;
use crate::hash::AddressHash;
use crate::identity::PrivateIdentity;
use crate::iface::{InterfaceManager, TxMessage, TxMessageType};
use crate::packet::{Packet, PacketDataBuffer};
use crate::serde::Serialize;

use super::*;

#[path = "endpoint_lifecycle.rs"]
mod endpoint_lifecycle;

#[path = "blocked_shutdown.rs"]
mod blocked_shutdown;

fn packet_payload(packet: &Packet) -> Vec<u8> {
    let mut buffer = vec![0_u8; 512];
    let mut output = OutputBuffer::new(&mut buffer);
    packet.serialize(&mut output).expect("serialize packet");
    output.as_slice().to_vec()
}

fn valid_test_packet() -> Packet {
    Packet { data: PacketDataBuffer::new_from_slice(b"weave-test"), ..Packet::default() }
}

fn log_frame(target: [u8; 4], event: u16, data: &[u8]) -> Vec<u8> {
    let mut payload = vec![0, 0, 0, 0, 0, 0];
    payload.extend_from_slice(&event.to_be_bytes());
    payload.extend_from_slice(data);
    weave_wire_frame(&weave_wdcl_frame(target, WDCL_T_LOG, &payload))
}

fn display_frame(
    target: [u8; 4],
    color_format: u8,
    offset: u32,
    total_size: u32,
    data: &[u8],
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(9 + data.len());
    payload.push(color_format);
    payload.extend_from_slice(&offset.to_be_bytes());
    payload.extend_from_slice(&total_size.to_be_bytes());
    payload.extend_from_slice(data);
    weave_wire_frame(&weave_wdcl_frame(target, WDCL_T_DISP, &payload))
}

fn decode_one_wire_frame(bytes: &[u8]) -> Vec<u8> {
    let (start, end) = Hdlc::find(bytes).expect("hdlc frame");
    let mut decoded = vec![0_u8; 4096];
    let mut output = OutputBuffer::new(&mut decoded);
    let len = Hdlc::decode(&bytes[start..=end], &mut output).expect("decode frame");
    output.as_slice()[..len].to_vec()
}

struct FailingWeaveWriteStream {
    read_chunks: VecDeque<Vec<u8>>,
    successful_writes: usize,
    write_attempts: usize,
    successful_flushes: usize,
    flush_attempts: usize,
}

impl FailingWeaveWriteStream {
    fn new(read_chunks: Vec<Vec<u8>>, successful_writes: usize) -> Self {
        Self {
            read_chunks: read_chunks.into(),
            successful_writes,
            write_attempts: 0,
            successful_flushes: usize::MAX,
            flush_attempts: 0,
        }
    }

    fn with_successful_flushes(mut self, successful_flushes: usize) -> Self {
        self.successful_flushes = successful_flushes;
        self
    }
}

impl AsyncRead for FailingWeaveWriteStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let Some(mut chunk) = self.read_chunks.pop_front() else {
            return Poll::Pending;
        };
        let len = chunk.len().min(buf.remaining());
        buf.put_slice(&chunk[..len]);
        if len < chunk.len() {
            chunk.drain(..len);
            self.read_chunks.push_front(chunk);
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for FailingWeaveWriteStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.write_attempts = self.write_attempts.saturating_add(1);
        if self.write_attempts > self.successful_writes {
            return Poll::Ready(Err(io::Error::other("synthetic weave command write failure")));
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.flush_attempts = self.flush_attempts.saturating_add(1);
        if self.flush_attempts > self.successful_flushes {
            return Poll::Ready(Err(io::Error::other("synthetic weave flush failure")));
        }
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn weave_stream_discovery_flush_failure_exits_for_reconnect() {
    let (options, _manager, _parent) = test_options().await;
    let runtime_status = options.runtime_status.clone();
    let stream = FailingWeaveWriteStream::new(Vec::new(), usize::MAX).with_successful_flushes(0);
    let (rx_tx, _rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);

    tokio::time::timeout(
        Duration::from_secs(1),
        run_weave_stream(
            stream,
            options,
            CancellationToken::new(),
            CancellationToken::new(),
            rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
            unused_weave_management_rx(),
        ),
    )
    .await
    .expect("weave stream exits after discovery flush failure");

    let status = runtime_status.lock().expect("weave runtime status").clone();
    assert_eq!(status.link_state, WeaveLinkState::Reconnecting);
    assert!(
        status
            .last_error
            .as_deref()
            .is_some_and(|err| err.contains("synthetic weave flush failure")),
        "unexpected last_error {:?}",
        status.last_error
    );
}

async fn test_options(
) -> (WeaveStreamOptions, Arc<tokio::sync::Mutex<InterfaceManager>>, AddressHash) {
    let mut manager = InterfaceManager::new(8);
    let channel = manager.new_channel_with_role(8, IfaceRole::Multicast);
    let parent = channel.address;
    let manager = Arc::new(tokio::sync::Mutex::new(manager));
    let switch_identity = PrivateIdentity::new_from_name("weave-test");
    let runtime_status = Arc::new(std::sync::Mutex::new(WeaveRuntimeStatus::new(
        "test".to_string(),
        DEFAULT_BAUD_RATE,
        DEFAULT_MTU,
        switch_id_for_identity(&switch_identity),
    )));
    (
        WeaveStreamOptions {
            parent_iface: parent,
            device: "test".to_string(),
            mtu: DEFAULT_MTU,
            iface_manager: manager.clone(),
            switch_identity,
            runtime_status,
        },
        manager,
        parent,
    )
}

fn unused_weave_management_rx() -> WeaveManagementFrameReceiver {
    let (_tx, rx) = weave_management_channel();
    rx
}

#[tokio::test]
async fn weave_endpoint_gc_removes_stale_virtual_iface_and_status() {
    let (options, manager, _parent) = test_options().await;
    let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
    let endpoint = [0x11_u8; ENDPOINT_ID_LEN];
    let address =
        ensure_weave_endpoint(endpoint, &options, state.clone()).await.expect("register endpoint");
    update_weave_status(&options.runtime_status, |status| {
        status.mark_endpoint_alive(endpoint, address);
    });
    {
        let mut state = state.lock().await;
        state.endpoints.get_mut(&endpoint).expect("endpoint state").last_seen =
            Instant::now() - WEAVE_ENDPOINT_IDLE_TIMEOUT - Duration::from_secs(1);
    }

    gc_weave_endpoints(&options, state.clone(), Instant::now()).await;

    let state = state.lock().await;
    assert!(state.endpoints.is_empty());
    assert!(state.addresses.is_empty());
    drop(state);
    assert!(options.runtime_status.lock().expect("weave runtime status").endpoints.is_empty());
    assert_eq!(manager.lock().await.role(&address), None);
}

#[tokio::test]
async fn weave_endpoint_gc_keeps_fresh_endpoint() {
    let (options, manager, _parent) = test_options().await;
    let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
    let endpoint = [0x22_u8; ENDPOINT_ID_LEN];
    let address =
        ensure_weave_endpoint(endpoint, &options, state.clone()).await.expect("register endpoint");
    update_weave_status(&options.runtime_status, |status| {
        status.mark_endpoint_alive(endpoint, address);
    });

    gc_weave_endpoints(&options, state.clone(), Instant::now()).await;

    assert!(state.lock().await.endpoints.contains_key(&endpoint));
    assert!(options
        .runtime_status
        .lock()
        .expect("weave runtime status")
        .endpoints
        .contains_key(&endpoint));
    assert_eq!(manager.lock().await.role(&address), Some(IfaceRole::VirtualUnicast));
}

#[tokio::test]
async fn weave_endpoint_repeat_event_refreshes_last_seen_and_reuses_iface() {
    let (options, manager, _parent) = test_options().await;
    let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
    let endpoint = [0x33_u8; ENDPOINT_ID_LEN];
    let address =
        ensure_weave_endpoint(endpoint, &options, state.clone()).await.expect("register endpoint");
    {
        let mut state = state.lock().await;
        state.endpoints.get_mut(&endpoint).expect("endpoint state").last_seen =
            Instant::now() - WEAVE_ENDPOINT_IDLE_TIMEOUT - Duration::from_secs(1);
    }

    let refreshed_address =
        ensure_weave_endpoint(endpoint, &options, state.clone()).await.expect("refresh endpoint");
    gc_weave_endpoints(&options, state.clone(), Instant::now()).await;

    assert_eq!(refreshed_address, address);
    assert!(state.lock().await.endpoints.contains_key(&endpoint));
    assert_eq!(manager.lock().await.role(&address), Some(IfaceRole::VirtualUnicast));
}

#[tokio::test]
async fn weave_tx_frames_refreshes_endpoint_activity() {
    let (options, manager, _parent) = test_options().await;
    let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
    let ifac_state = Arc::new(std::sync::RwLock::new(None));
    let direct_endpoint = [0x34_u8; ENDPOINT_ID_LEN];
    let stale_endpoint = [0x35_u8; ENDPOINT_ID_LEN];
    let direct_address = ensure_weave_endpoint(direct_endpoint, &options, state.clone())
        .await
        .expect("register direct endpoint");
    let stale_address = ensure_weave_endpoint(stale_endpoint, &options, state.clone())
        .await
        .expect("register stale endpoint");
    {
        let mut state = state.lock().await;
        state.remote_switch_id = Some([0x99_u8; SWITCH_ID_LEN]);
        for endpoint in [direct_endpoint, stale_endpoint] {
            state.endpoints.get_mut(&endpoint).expect("endpoint state").last_seen =
                Instant::now() - WEAVE_ENDPOINT_IDLE_TIMEOUT - Duration::from_secs(1);
        }
    }

    let frames = weave_tx_frames(
        &TxMessage { tx_type: TxMessageType::Direct(direct_address), packet: Packet::default() },
        &options,
        state.clone(),
        &ifac_state,
    )
    .await
    .expect("direct tx frame");
    assert_eq!(frames.len(), 1);
    gc_weave_endpoints(&options, state.clone(), Instant::now()).await;

    assert!(state.lock().await.endpoints.contains_key(&direct_endpoint));
    assert_eq!(manager.lock().await.role(&direct_address), Some(IfaceRole::VirtualUnicast));
    assert_eq!(manager.lock().await.role(&stale_address), None);

    let broadcast_endpoint = [0x36_u8; ENDPOINT_ID_LEN];
    let broadcast_address = ensure_weave_endpoint(broadcast_endpoint, &options, state.clone())
        .await
        .expect("register broadcast endpoint");
    {
        let mut state = state.lock().await;
        state.endpoints.get_mut(&broadcast_endpoint).expect("endpoint state").last_seen =
            Instant::now() - WEAVE_ENDPOINT_IDLE_TIMEOUT - Duration::from_secs(1);
    }

    let frames = weave_tx_frames(
        &TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() },
        &options,
        state.clone(),
        &ifac_state,
    )
    .await
    .expect("broadcast tx frames");
    assert_eq!(frames.len(), 2);
    gc_weave_endpoints(&options, state.clone(), Instant::now()).await;

    assert!(state.lock().await.endpoints.contains_key(&broadcast_endpoint));
    assert_eq!(manager.lock().await.role(&broadcast_address), Some(IfaceRole::VirtualUnicast));
}

#[tokio::test]
async fn weave_stream_shutdown_stops_registered_endpoint_virtual_ifaces() {
    let (options, manager, _parent) = test_options().await;
    let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
    let first_endpoint = [0x44_u8; ENDPOINT_ID_LEN];
    let second_endpoint = [0x55_u8; ENDPOINT_ID_LEN];
    let first_address = ensure_weave_endpoint(first_endpoint, &options, state.clone())
        .await
        .expect("register first endpoint");
    let second_address = ensure_weave_endpoint(second_endpoint, &options, state.clone())
        .await
        .expect("register second endpoint");
    update_weave_status(&options.runtime_status, |status| {
        status.mark_endpoint_alive(first_endpoint, first_address);
        status.mark_endpoint_via(second_endpoint, second_address);
    });

    cleanup_weave_endpoints(&options, state.clone()).await;

    let state = state.lock().await;
    assert!(state.endpoints.is_empty());
    assert!(state.addresses.is_empty());
    drop(state);
    assert!(options.runtime_status.lock().expect("weave runtime status").endpoints.is_empty());
    let manager = manager.lock().await;
    assert_eq!(manager.role(&first_address), None);
    assert_eq!(manager.role(&second_address), None);
}

#[tokio::test]
async fn weave_stream_cancel_marks_runtime_closed_without_hardware() {
    let (options, manager, _parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let runtime_status = options.runtime_status.clone();
    let endpoint = [0x47_u8; ENDPOINT_ID_LEN];
    let (stream, mut peer) = duplex(8192);
    let (rx_tx, _rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
    ));

    let mut drain = vec![0_u8; 128];
    let _ = peer.read(&mut drain).await.expect("discover frame");
    peer.write_all(&log_frame(local_switch, ET_PROTO_WEAVE_EP_ALIVE, &endpoint))
        .await
        .expect("alive event");
    let endpoint_iface = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let endpoint_iface = {
                let status = runtime_status.lock().expect("weave runtime status");
                status.endpoints.get(&endpoint).map(|endpoint| endpoint.iface)
            };
            if let Some(endpoint_iface) = endpoint_iface {
                break endpoint_iface;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("endpoint registered");
    assert_eq!(manager.lock().await.role(&endpoint_iface), Some(IfaceRole::VirtualUnicast));

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("weave stream shutdown timeout")
        .expect("weave stream task");

    let status = runtime_status.lock().expect("weave runtime status").clone();
    assert_eq!(status.link_state, WeaveLinkState::Closed);
    assert!(!status.wdcl_connected);
    assert!(status.last_error.is_none());
    assert!(status.endpoints.is_empty());
    assert_eq!(manager.lock().await.role(&endpoint_iface), None);
}

#[tokio::test]
async fn weave_configured_stream_exchanges_endpoint_packet_and_cleans_up() {
    let mut iface_manager = InterfaceManager::new(8);
    let parent_channel = iface_manager.new_channel_with_role(8, IfaceRole::Multicast);
    let parent_iface = parent_channel.address;
    let iface_manager = Arc::new(tokio::sync::Mutex::new(iface_manager));
    let iface = WeaveInterface::new("weave-loopback", iface_manager.clone())
        .with_baud_rate(115_200)
        .with_mtu(512);
    let local_switch = iface.switch_id();
    let remote = PrivateIdentity::new_from_name("weave-loopback-peer");
    let remote_switch = switch_id_for_identity(&remote);
    let runtime_status = iface.runtime_status.clone();
    let options = WeaveStreamOptions {
        parent_iface,
        device: iface.device.clone(),
        mtu: iface.mtu,
        iface_manager: iface_manager.clone(),
        switch_identity: iface.switch_identity.clone(),
        runtime_status: runtime_status.clone(),
    };
    let (stream, mut peer) = duplex(8192);
    let (rx_tx, mut rx_rx) = tokio::sync::mpsc::channel(4);
    let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
    ));

    let mut wire = vec![0_u8; 512];
    let discover_len = peer.read(&mut wire).await.expect("read discovery frame");
    let discovery = decode_one_wire_frame(&wire[..discover_len]);
    assert_eq!(discovery[SWITCH_ID_LEN], WDCL_T_DISCOVER);
    assert_eq!(&discovery[..SWITCH_ID_LEN], &[0xFF; SWITCH_ID_LEN]);

    let mut discovery_payload = Vec::new();
    discovery_payload.extend_from_slice(remote.as_identity().verifying_key_bytes());
    discovery_payload.extend_from_slice(&remote.sign(&local_switch).to_bytes());
    peer.write_all(&weave_wire_frame(&weave_wdcl_frame(
        local_switch,
        WDCL_T_DISCOVER,
        &discovery_payload,
    )))
    .await
    .expect("respond to discovery");
    let handshake_len = peer.read(&mut wire).await.expect("read handshake");
    let handshake = decode_one_wire_frame(&wire[..handshake_len]);
    assert_eq!(handshake[SWITCH_ID_LEN], WDCL_T_CONNECT);

    let endpoint = [0x42_u8; ENDPOINT_ID_LEN];
    peer.write_all(&log_frame(local_switch, ET_PROTO_WEAVE_EP_ALIVE, &endpoint))
        .await
        .expect("announce loopback endpoint");
    let mut inbound_payload = packet_payload(&valid_test_packet());
    inbound_payload.extend_from_slice(&endpoint);
    peer.write_all(&weave_wire_frame(&weave_wdcl_frame(
        local_switch,
        WDCL_T_ENDPOINT_PKT,
        &inbound_payload,
    )))
    .await
    .expect("send inbound endpoint packet");
    let inbound = tokio::time::timeout(Duration::from_secs(1), rx_rx.recv())
        .await
        .expect("inbound packet timeout")
        .expect("inbound packet");
    assert_eq!(iface_manager.lock().await.role(&inbound.address), Some(IfaceRole::VirtualUnicast));

    tx_tx
        .send(TxMessage {
            tx_type: TxMessageType::Direct(inbound.address),
            packet: valid_test_packet(),
        })
        .await
        .expect("queue outbound endpoint packet");
    let outbound_len = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut wire))
        .await
        .expect("outbound packet timeout")
        .expect("read outbound endpoint packet");
    let outbound = decode_one_wire_frame(&wire[..outbound_len]);
    assert_eq!(&outbound[..SWITCH_ID_LEN], &remote_switch);
    assert_eq!(outbound[SWITCH_ID_LEN], WDCL_T_CMD);
    assert_eq!(
        &outbound[SWITCH_ID_LEN + 1..SWITCH_ID_LEN + 3],
        &WDCL_CMD_ENDPOINT_PKT.to_be_bytes()
    );
    assert_eq!(
        &outbound[SWITCH_ID_LEN + 1 + 2..SWITCH_ID_LEN + 1 + 2 + ENDPOINT_ID_LEN],
        &endpoint
    );
    assert_eq!(
        &outbound[SWITCH_ID_LEN + 1 + 2 + ENDPOINT_ID_LEN..],
        packet_payload(&valid_test_packet())
    );

    let status = runtime_status.lock().expect("Weave status").clone();
    let endpoint_status = status.endpoints.get(&endpoint).expect("endpoint status");
    assert_eq!(status.remote_switch_id, Some(remote_switch));
    assert_eq!(endpoint_status.packets_rx, 1);
    assert_eq!(status.frames_tx, 3);
    assert!(status.bytes_rx > 0);
    assert!(status.bytes_tx > 0);

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("Weave stream shutdown timeout")
        .expect("Weave stream task");
    let status = runtime_status.lock().expect("Weave closed status").clone();
    assert_eq!(status.link_state, WeaveLinkState::Closed);
    assert!(status.endpoints.is_empty());
    assert_eq!(iface_manager.lock().await.role(&inbound.address), None);
}

#[tokio::test]
async fn weave_stream_routes_inbound_endpoint_packet_to_virtual_iface() {
    let (options, _manager, parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let runtime_status = options.runtime_status.clone();
    let endpoint = [0x42_u8; ENDPOINT_ID_LEN];
    let (stream, mut peer) = duplex(8192);
    let (rx_tx, mut rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
    ));

    let mut drain = vec![0_u8; 128];
    let _ = peer.read(&mut drain).await.expect("discover frame");
    peer.write_all(&log_frame(local_switch, ET_PROTO_WEAVE_EP_ALIVE, &endpoint))
        .await
        .expect("alive event");
    let mut endpoint_payload = packet_payload(&valid_test_packet());
    endpoint_payload.extend_from_slice(&endpoint);
    peer.write_all(&weave_wire_frame(&weave_wdcl_frame(
        local_switch,
        WDCL_T_ENDPOINT_PKT,
        &endpoint_payload,
    )))
    .await
    .expect("endpoint packet");
    let message = rx_rx.recv().await.expect("rx message");
    let status = runtime_status.lock().expect("weave runtime status").clone();
    cancel.cancel();
    task.await.expect("stream task");

    assert_ne!(message.address, parent);
    assert_eq!(status.endpoints.len(), 1);
    let endpoint_status = status.endpoints.get(&endpoint).expect("endpoint status");
    assert_eq!(endpoint_status.alive_events, 1);
    assert_eq!(endpoint_status.packets_rx, 1);
    assert_eq!(status.last_log_event, Some(ET_PROTO_WEAVE_EP_ALIVE));
}

include!("../../weave_ifac_runtime_tests.rs");

#[tokio::test]
async fn weave_stream_sends_handshake_for_valid_discovery_response() {
    let (options, _manager, _parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let runtime_status = options.runtime_status.clone();
    let remote = PrivateIdentity::new_from_name("remote-weave");
    let mut discovery_payload = Vec::new();
    discovery_payload.extend_from_slice(remote.as_identity().verifying_key_bytes());
    discovery_payload.extend_from_slice(&remote.sign(&local_switch).to_bytes());

    let (stream, mut peer) = duplex(8192);
    let (rx_tx, _rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
    ));

    let mut bytes = vec![0_u8; 512];
    let _ = peer.read(&mut bytes).await.expect("discover frame");
    peer.write_all(&weave_wire_frame(&weave_wdcl_frame(
        local_switch,
        WDCL_T_DISCOVER,
        &discovery_payload,
    )))
    .await
    .expect("discovery response");
    let n = peer.read(&mut bytes).await.expect("handshake frame");
    let status = runtime_status.lock().expect("weave runtime status").clone();
    cancel.cancel();
    task.await.expect("stream task");

    let handshake = decode_one_wire_frame(&bytes[..n]);
    assert_eq!(handshake[..SWITCH_ID_LEN], switch_id_for_identity(&remote));
    assert_eq!(handshake[SWITCH_ID_LEN], WDCL_T_CONNECT);
    assert_eq!(status.remote_switch_id, Some(switch_id_for_identity(&remote)));
    assert_eq!(status.frames_tx, 2);
}

#[tokio::test]
async fn weave_stream_slices_first_frame_after_leading_serial_noise() {
    let (options, _manager, _parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let runtime_status = options.runtime_status.clone();
    let remote = PrivateIdentity::new_from_name("remote-weave-noise");
    let mut discovery_payload = Vec::new();
    discovery_payload.extend_from_slice(remote.as_identity().verifying_key_bytes());
    discovery_payload.extend_from_slice(&remote.sign(&local_switch).to_bytes());

    let (stream, mut peer) = duplex(8192);
    let (rx_tx, _rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
    ));

    let mut bytes = vec![0_u8; 512];
    let _ = peer.read(&mut bytes).await.expect("discover frame");
    peer.write_all(b"boot log before hdlc").await.expect("leading serial noise");
    peer.write_all(&weave_wire_frame(&weave_wdcl_frame(
        local_switch,
        WDCL_T_DISCOVER,
        &discovery_payload,
    )))
    .await
    .expect("discovery response");
    let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
        .await
        .expect("handshake frame timeout")
        .expect("handshake frame");
    let status = runtime_status.lock().expect("weave runtime status").clone();
    cancel.cancel();
    task.await.expect("stream task");

    let handshake = decode_one_wire_frame(&bytes[..n]);
    assert_eq!(handshake[..SWITCH_ID_LEN], switch_id_for_identity(&remote));
    assert_eq!(handshake[SWITCH_ID_LEN], WDCL_T_CONNECT);
    assert_eq!(status.remote_switch_id, Some(switch_id_for_identity(&remote)));
    assert_eq!(status.invalid_frames, 0);
}

#[test]
fn weave_remote_display_command_frames_match_python_wdcl_control() {
    let remote_switch = [0x10, 0x20, 0x30, 0x40];

    let enable = weave_remote_display_command_frame(remote_switch, true);
    let disable = weave_remote_display_command_frame(remote_switch, false);

    for (frame, expected_value) in [(enable, 1_u8), (disable, 0_u8)] {
        let wire = weave_wire_frame(&frame);
        let command = decode_one_wire_frame(&wire);
        assert_eq!(command[..SWITCH_ID_LEN], remote_switch);
        assert_eq!(command[SWITCH_ID_LEN], WDCL_T_CMD);
        assert_eq!(
            &command[SWITCH_ID_LEN + 1..SWITCH_ID_LEN + 3],
            &WDCL_CMD_REMOTE_DISPLAY.to_be_bytes()
        );
        assert_eq!(command[SWITCH_ID_LEN + 3], expected_value);
        assert_eq!(command.len(), SWITCH_ID_LEN + 1 + 3);
    }
}

#[tokio::test]
async fn weave_management_handle_writes_remote_display_control_frames() {
    let (options, _manager, _parent) = test_options().await;
    let runtime_status = options.runtime_status.clone();
    let learned_switch = [0x10, 0x20, 0x30, 0x40];
    let explicit_switch = [0x50, 0x60, 0x70, 0x80];
    runtime_status.lock().expect("weave runtime status").remote_switch_id = Some(learned_switch);
    let (management_tx, management_rx) = weave_management_channel();
    let handle =
        WeaveManagementHandle { tx: management_tx, runtime_status: runtime_status.clone() };

    let (stream, mut peer) = duplex(8192);
    let (rx_tx, _rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        management_rx,
    ));

    let mut bytes = vec![0_u8; 512];
    let _ = peer.read(&mut bytes).await.expect("discover frame");

    let used_switch =
        handle.try_set_remote_display(None, true).expect("queue learned switch enable");
    assert_eq!(used_switch, learned_switch);
    let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
        .await
        .expect("enable frame timeout")
        .expect("enable frame");
    let enable = decode_one_wire_frame(&bytes[..n]);
    assert_eq!(enable[..SWITCH_ID_LEN], learned_switch);
    assert_eq!(enable[SWITCH_ID_LEN], WDCL_T_CMD);
    assert_eq!(
        &enable[SWITCH_ID_LEN + 1..SWITCH_ID_LEN + 3],
        &WDCL_CMD_REMOTE_DISPLAY.to_be_bytes()
    );
    assert_eq!(enable[SWITCH_ID_LEN + 3], 1);

    let used_switch = handle
        .try_set_remote_display(Some(explicit_switch), false)
        .expect("queue explicit switch disable");
    assert_eq!(used_switch, explicit_switch);
    let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
        .await
        .expect("disable frame timeout")
        .expect("disable frame");
    let disable = decode_one_wire_frame(&bytes[..n]);
    assert_eq!(disable[..SWITCH_ID_LEN], explicit_switch);
    assert_eq!(disable[SWITCH_ID_LEN], WDCL_T_CMD);
    assert_eq!(
        &disable[SWITCH_ID_LEN + 1..SWITCH_ID_LEN + 3],
        &WDCL_CMD_REMOTE_DISPLAY.to_be_bytes()
    );
    assert_eq!(disable[SWITCH_ID_LEN + 3], 0);

    let status = runtime_status.lock().expect("weave runtime status").clone();
    cancel.cancel();
    task.await.expect("stream task");
    assert!(status.frames_tx >= 3);
    assert!(status.bytes_tx > 0);
}

#[tokio::test]
async fn weave_stream_routes_direct_tx_to_endpoint_command() {
    let (options, _manager, _parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let runtime_status = options.runtime_status.clone();
    let remote = PrivateIdentity::new_from_name("remote-weave-direct");
    let remote_switch = switch_id_for_identity(&remote);
    let endpoint = [0x24_u8; ENDPOINT_ID_LEN];
    let mut discovery_payload = Vec::new();
    discovery_payload.extend_from_slice(remote.as_identity().verifying_key_bytes());
    discovery_payload.extend_from_slice(&remote.sign(&local_switch).to_bytes());

    let (stream, mut peer) = duplex(8192);
    let (rx_tx, mut rx_rx) = tokio::sync::mpsc::channel(4);
    let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
    ));

    let mut bytes = vec![0_u8; 4096];
    let _ = peer.read(&mut bytes).await.expect("discover frame");
    peer.write_all(&weave_wire_frame(&weave_wdcl_frame(
        local_switch,
        WDCL_T_DISCOVER,
        &discovery_payload,
    )))
    .await
    .expect("discovery response");
    let _ = peer.read(&mut bytes).await.expect("handshake frame");
    let mut endpoint_payload = packet_payload(&valid_test_packet());
    endpoint_payload.extend_from_slice(&endpoint);
    peer.write_all(&weave_wire_frame(&weave_wdcl_frame(
        local_switch,
        WDCL_T_ENDPOINT_PKT,
        &endpoint_payload,
    )))
    .await
    .expect("inbound packet");
    let message = rx_rx.recv().await.expect("rx message");
    peer.write_all(&log_frame(local_switch, ET_PROTO_WDCL_CONNECTION, &[]))
        .await
        .expect("connection event");
    tx_tx
        .send(TxMessage {
            tx_type: TxMessageType::Direct(message.address),
            packet: Packet::default(),
        })
        .await
        .expect("queue tx");
    let n = peer.read(&mut bytes).await.expect("endpoint command");
    let status = runtime_status.lock().expect("weave runtime status").clone();
    cancel.cancel();
    task.await.expect("stream task");

    let command = decode_one_wire_frame(&bytes[..n]);
    assert_eq!(command[..SWITCH_ID_LEN], remote_switch);
    assert_eq!(command[SWITCH_ID_LEN], WDCL_T_CMD);
    assert_eq!(
        &command[SWITCH_ID_LEN + 1..SWITCH_ID_LEN + 3],
        &WDCL_CMD_ENDPOINT_PKT.to_be_bytes()
    );
    assert_eq!(&command[SWITCH_ID_LEN + 3..SWITCH_ID_LEN + 3 + ENDPOINT_ID_LEN], &endpoint);
    assert_eq!(status.link_state, WeaveLinkState::Connected);
    assert!(status.wdcl_connected);
    assert!(status.bytes_tx > 0);
    assert!(status.bytes_rx > 0);
}

#[tokio::test]
async fn weave_stream_handshake_write_failure_exits_for_reconnect() {
    let (options, _manager, _parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let runtime_status = options.runtime_status.clone();
    let remote = PrivateIdentity::new_from_name("remote-weave-handshake-failure");
    let mut discovery_payload = Vec::new();
    discovery_payload.extend_from_slice(remote.as_identity().verifying_key_bytes());
    discovery_payload.extend_from_slice(&remote.sign(&local_switch).to_bytes());
    let stream = FailingWeaveWriteStream::new(
        vec![weave_wire_frame(&weave_wdcl_frame(
            local_switch,
            WDCL_T_DISCOVER,
            &discovery_payload,
        ))],
        1,
    );
    let (rx_tx, _rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();

    tokio::time::timeout(
        Duration::from_secs(1),
        run_weave_stream(
            stream,
            options,
            cancel,
            CancellationToken::new(),
            rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
            unused_weave_management_rx(),
        ),
    )
    .await
    .expect("weave stream exits after handshake write failure");

    let status = runtime_status.lock().expect("weave runtime status").clone();
    assert_eq!(status.link_state, WeaveLinkState::Reconnecting);
    assert!(!status.wdcl_connected);
    assert!(
        status
            .last_error
            .as_deref()
            .is_some_and(|err| err.contains("synthetic weave command write failure")),
        "unexpected last_error {:?}",
        status.last_error
    );
    assert!(status.endpoints.is_empty());
}

#[tokio::test]
async fn weave_stream_command_write_failure_exits_for_reconnect() {
    let (options, _manager, _parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let runtime_status = options.runtime_status.clone();
    let remote = PrivateIdentity::new_from_name("remote-weave-write-failure");
    let endpoint = [0x25_u8; ENDPOINT_ID_LEN];
    let mut discovery_payload = Vec::new();
    discovery_payload.extend_from_slice(remote.as_identity().verifying_key_bytes());
    discovery_payload.extend_from_slice(&remote.sign(&local_switch).to_bytes());
    let mut endpoint_payload = packet_payload(&valid_test_packet());
    endpoint_payload.extend_from_slice(&endpoint);
    let stream = FailingWeaveWriteStream::new(
        vec![
            weave_wire_frame(&weave_wdcl_frame(local_switch, WDCL_T_DISCOVER, &discovery_payload)),
            weave_wire_frame(&weave_wdcl_frame(
                local_switch,
                WDCL_T_ENDPOINT_PKT,
                &endpoint_payload,
            )),
            log_frame(local_switch, ET_PROTO_WDCL_CONNECTION, &[]),
        ],
        2,
    );
    let (rx_tx, mut rx_rx) = tokio::sync::mpsc::channel(4);
    let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel,
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
    ));

    let message = tokio::time::timeout(Duration::from_secs(1), rx_rx.recv())
        .await
        .expect("endpoint rx timeout")
        .expect("endpoint rx");
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let connected = {
                let status = runtime_status.lock().expect("weave runtime status");
                status.link_state == WeaveLinkState::Connected && status.wdcl_connected
            };
            if connected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("runtime connected");

    tx_tx
        .send(TxMessage {
            tx_type: TxMessageType::Direct(message.address),
            packet: Packet::default(),
        })
        .await
        .expect("queue tx");

    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("weave stream write failure timeout")
        .expect("weave stream task");

    let status = runtime_status.lock().expect("weave runtime status").clone();
    assert_eq!(status.link_state, WeaveLinkState::Reconnecting);
    assert!(!status.wdcl_connected);
    assert!(
        status
            .last_error
            .as_deref()
            .is_some_and(|err| err.contains("synthetic weave command write failure")),
        "unexpected last_error {:?}",
        status.last_error
    );
    assert!(status.endpoints.is_empty());
}

#[tokio::test]
async fn weave_runtime_status_json_exposes_display_and_device_stats_fields() {
    let (options, _manager, _parent) = test_options().await;
    let json = options.runtime_status.lock().expect("weave runtime status").to_json();

    assert_eq!(json["device"].as_str(), Some("test"));
    assert_eq!(json["baud_rate"].as_u64(), Some(DEFAULT_BAUD_RATE as u64));
    assert_eq!(json["mtu"].as_u64(), Some(DEFAULT_MTU as u64));
    assert_eq!(json["link_state"].as_str(), Some("configured"));
    assert_eq!(json["endpoint_count"].as_u64(), Some(0));
    assert!(json["display"].is_null());
    assert!(json["device_stats"].is_null());
}

#[tokio::test]
async fn weave_stream_captures_display_and_stat_log_events() {
    let (options, _manager, _parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let runtime_status = options.runtime_status.clone();
    let (stream, mut peer) = duplex(8192);
    let (rx_tx, _rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
    ));

    let mut drain = vec![0_u8; 128];
    let _ = peer.read(&mut drain).await.expect("discover frame");
    peer.write_all(&display_frame(local_switch, 1, 0, 4, &[0xAA, 0xBB]))
        .await
        .expect("display first chunk");
    peer.write_all(&display_frame(local_switch, 1, 2, 4, &[0xCC, 0xDD]))
        .await
        .expect("display second chunk");
    peer.write_all(&log_frame(local_switch, ET_STAT_CPU, &[73])).await.expect("cpu stat");
    peer.write_all(&log_frame(local_switch, ET_STAT_TASK_CPU, &[12, b'w', b'o', b'r', b'k']))
        .await
        .expect("task cpu stat");
    let mut memory = Vec::new();
    memory.extend_from_slice(&4096_u32.to_be_bytes());
    memory.extend_from_slice(&16384_u32.to_be_bytes());
    peer.write_all(&log_frame(local_switch, ET_STAT_MEMORY, &memory)).await.expect("memory stat");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let json = runtime_status.lock().expect("weave runtime status").to_json();
    cancel.cancel();
    task.await.expect("stream task");

    assert_eq!(json["display"]["color_format"].as_u64(), Some(1));
    assert_eq!(json["display"]["width"].as_u64(), Some(128));
    assert_eq!(json["display"]["height"].as_u64(), Some(64));
    assert_eq!(json["display"]["total_size"].as_u64(), Some(4));
    assert_eq!(json["display"]["received_size"].as_u64(), Some(4));
    assert_eq!(json["display"]["complete"].as_bool(), Some(true));
    assert_eq!(json["display"]["buffer_hex"].as_str(), Some("aabbccdd"));
    assert_eq!(json["device_stats"]["cpu_load"].as_u64(), Some(73));
    assert_eq!(json["device_stats"]["memory_free"].as_u64(), Some(4096));
    assert_eq!(json["device_stats"]["memory_total"].as_u64(), Some(16384));
    assert_eq!(json["device_stats"]["memory_used"].as_u64(), Some(12288));
    assert_eq!(json["device_stats"]["memory_used_percent_bp"].as_u64(), Some(7500));
    assert_eq!(json["device_stats"]["task_cpu"]["work"]["cpu_load"].as_u64(), Some(12));
    assert_eq!(json["device_stats"]["task_cpu"]["work"]["samples"].as_u64(), Some(1));
}

#[tokio::test]
async fn weave_display_status_requires_full_byte_coverage_before_complete() {
    let (options, _manager, _parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let runtime_status = options.runtime_status.clone();
    let (stream, mut peer) = duplex(8192);
    let (rx_tx, _rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
    ));

    let mut drain = vec![0_u8; 128];
    let _ = peer.read(&mut drain).await.expect("discover frame");
    peer.write_all(&display_frame(local_switch, 1, 2, 4, &[0xCC, 0xDD]))
        .await
        .expect("display tail chunk");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let partial_json = runtime_status.lock().expect("weave runtime status").to_json();
    peer.write_all(&display_frame(local_switch, 1, 0, 4, &[0xAA, 0xBB]))
        .await
        .expect("display head chunk");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let complete_json = runtime_status.lock().expect("weave runtime status").to_json();
    cancel.cancel();
    task.await.expect("stream task");

    assert_eq!(partial_json["display"]["received_size"].as_u64(), Some(2));
    assert_eq!(partial_json["display"]["complete"].as_bool(), Some(false));
    assert!(partial_json["display"]["buffer_hex"].is_null());
    assert_eq!(complete_json["display"]["received_size"].as_u64(), Some(4));
    assert_eq!(complete_json["display"]["complete"].as_bool(), Some(true));
    assert_eq!(complete_json["display"]["buffer_hex"].as_str(), Some("aabbccdd"));
}

#[tokio::test]
async fn weave_stream_ignores_off_target_display_and_log_frames() {
    let (options, _manager, _parent) = test_options().await;
    let local_switch = switch_id_for_identity(&options.switch_identity);
    let mut off_target = local_switch;
    off_target[0] ^= 0xFF;
    let runtime_status = options.runtime_status.clone();
    let (stream, mut peer) = duplex(8192);
    let (rx_tx, _rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_weave_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
        unused_weave_management_rx(),
    ));

    let mut drain = vec![0_u8; 128];
    let _ = peer.read(&mut drain).await.expect("discover frame");
    peer.write_all(&display_frame(off_target, 1, 0, 4, &[0xAA, 0xBB, 0xCC, 0xDD]))
        .await
        .expect("off-target display");
    peer.write_all(&log_frame(off_target, ET_PROTO_WDCL_CONNECTION, &[]))
        .await
        .expect("off-target connection event");
    peer.write_all(&log_frame(off_target, ET_STAT_CPU, &[73])).await.expect("off-target cpu stat");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let status = runtime_status.lock().expect("weave runtime status").clone();
    cancel.cancel();
    task.await.expect("stream task");

    assert!(status.display.is_none());
    assert_eq!(status.device_stats, WeaveDeviceStats::default());
    assert!(status.log_events.is_empty());
    assert_eq!(status.last_log_event, None);
    assert_eq!(status.link_state, WeaveLinkState::Discovering);
    assert!(!status.wdcl_connected);
}
