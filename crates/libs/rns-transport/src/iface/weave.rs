use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use ed25519_dalek::Signature;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::Instant;
use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};
use tokio_util::sync::CancellationToken;

use crate::buffer::OutputBuffer;
use crate::hash::AddressHash;
use crate::identity::{Identity, PrivateIdentity};
use crate::iface::hdlc::Hdlc;
use crate::iface::{
    decode_packet_ifac, encode_packet_ifac, is_ifac_violation, record_ifac_violation, IfacRuntime,
    IfacState, MAX_IFAC_SIZE_BYTES,
};

use super::{
    IfaceRole, IfaceSource, Interface, InterfaceContext, InterfaceManager, RxMessage, TxMessage,
    TxMessageType,
};

const WDCL_T_DISCOVER: u8 = 0x00;
const WDCL_T_CONNECT: u8 = 0x01;
const WDCL_T_CMD: u8 = 0x02;
const WDCL_T_LOG: u8 = 0x03;
const WDCL_T_DISP: u8 = 0x04;
const WDCL_T_ENDPOINT_PKT: u8 = 0x05;
const WDCL_BROADCAST: [u8; 4] = [0xFF; 4];
const WDCL_CMD_ENDPOINT_PKT: u16 = 0x0001;
const WDCL_CMD_REMOTE_DISPLAY: u16 = 0x0A00;
const ET_PROTO_WDCL_CONNECTION: u16 = 0x3002;
const ET_PROTO_WDCL_HOST_ENDPOINT: u16 = 0x3003;
const ET_PROTO_WEAVE_EP_ALIVE: u16 = 0x3102;
const ET_PROTO_WEAVE_EP_VIA: u16 = 0x3104;
const ET_STAT_CPU: u16 = 0xE003;
const ET_STAT_TASK_CPU: u16 = 0xE004;
const ET_STAT_MEMORY: u16 = 0xE005;

const SWITCH_ID_LEN: usize = 4;
const ENDPOINT_ID_LEN: usize = 8;
const WEAVE_PUBKEY_SIZE: usize = 32;
const WEAVE_SIGNATURE_LEN: usize = 64;
const DEFAULT_BAUD_RATE: u32 = 3_000_000;
const DEFAULT_MTU: usize = 1024;
const READ_CAPACITY: usize = 1500;
const RECONNECT_WAIT: Duration = Duration::from_secs(5);
const WEAVE_ENDPOINT_IDLE_TIMEOUT: Duration = Duration::from_secs(1800);
const WEAVE_ENDPOINT_CLEANUP_INTERVAL: Duration = Duration::from_secs(10);
const WEAVE_MANAGEMENT_CHANNEL_CAPACITY: usize = 16;

type WeaveManagementFrameSender = tokio::sync::mpsc::Sender<Vec<u8>>;
type WeaveManagementFrameReceiver = Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>;

#[derive(Clone)]
struct WeaveStreamControl {
    cancel: CancellationToken,
    iface_stop: CancellationToken,
}

#[derive(Clone)]
pub struct WeaveInterface {
    device: String,
    baud_rate: u32,
    mtu: usize,
    switch_identity: PrivateIdentity,
    runtime_status: Arc<std::sync::Mutex<WeaveRuntimeStatus>>,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    management_frame_tx: WeaveManagementFrameSender,
    management_frame_rx: WeaveManagementFrameReceiver,
}

impl WeaveInterface {
    #[must_use]
    pub fn new<T: Into<String>>(
        device: T,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        let device = device.into();
        let switch_identity = PrivateIdentity::new_from_name(&format!("weave:{device}"));
        let local_switch_id = switch_id_for_identity(&switch_identity);
        let (management_frame_tx, management_frame_rx) = weave_management_channel();
        Self {
            runtime_status: Arc::new(std::sync::Mutex::new(WeaveRuntimeStatus::new(
                device.clone(),
                DEFAULT_BAUD_RATE,
                DEFAULT_MTU,
                local_switch_id,
            ))),
            switch_identity,
            device,
            baud_rate: DEFAULT_BAUD_RATE,
            mtu: DEFAULT_MTU,
            iface_manager,
            management_frame_tx,
            management_frame_rx,
        }
    }

    #[must_use]
    pub fn with_baud_rate(mut self, baud_rate: u32) -> Self {
        self.baud_rate = baud_rate.max(1);
        self.runtime_status.lock().expect("weave runtime status mutex poisoned").baud_rate =
            self.baud_rate;
        self
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(1);
        self.runtime_status.lock().expect("weave runtime status mutex poisoned").mtu = self.mtu;
        self
    }

    #[must_use]
    pub fn device(&self) -> &str {
        &self.device
    }

    #[must_use]
    pub fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    #[must_use]
    pub fn mtu_value(&self) -> usize {
        self.mtu
    }

    #[must_use]
    pub fn switch_id(&self) -> [u8; SWITCH_ID_LEN] {
        switch_id_for_identity(&self.switch_identity)
    }

    #[must_use]
    pub fn runtime_status_json(&self) -> serde_json::Value {
        self.runtime_status.lock().expect("weave runtime status mutex poisoned").to_json()
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> WeaveRuntimeStatusHandle {
        WeaveRuntimeStatusHandle { inner: self.runtime_status.clone() }
    }

    #[must_use]
    pub fn weave_management_handle(&self) -> WeaveManagementHandle {
        WeaveManagementHandle {
            tx: self.management_frame_tx.clone(),
            runtime_status: self.runtime_status.clone(),
        }
    }

    pub fn preflight_open(&self) -> Result<(), String> {
        tokio_serial::new(self.device.clone(), self.baud_rate)
            .data_bits(DataBits::Eight)
            .parity(Parity::None)
            .stop_bits(StopBits::One)
            .flow_control(FlowControl::None)
            .open_native_async()
            .map(|_| ())
            .map_err(|err| {
                format!(
                    "weave preflight open failed device={} baud_rate={} err={}",
                    self.device, self.baud_rate, err
                )
            })
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let parent_iface = context.channel.address;
        let ifac_state = context.channel.ifac_state.clone();
        let ifac_violations = context.channel.ifac_violations.clone();
        let (
            device,
            baud_rate,
            mtu,
            switch_identity,
            runtime_status,
            iface_manager,
            management_frame_rx,
        ) = {
            let guard = context.inner.lock().expect("weave interface mutex poisoned");
            (
                guard.device.clone(),
                guard.baud_rate,
                guard.mtu,
                guard.switch_identity.clone(),
                guard.runtime_status.clone(),
                guard.iface_manager.clone(),
                guard.management_frame_rx.clone(),
            )
        };
        let online = context.channel.online.clone();
        online.store(false, std::sync::atomic::Ordering::Release);
        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        loop {
            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            let port = match tokio_serial::new(device.clone(), baud_rate)
                .data_bits(DataBits::Eight)
                .parity(Parity::None)
                .stop_bits(StopBits::One)
                .flow_control(FlowControl::None)
                .open_native_async()
            {
                Ok(port) => port,
                Err(err) => {
                    update_weave_status(&runtime_status, |status| {
                        status.mark_reconnecting(format!(
                            "weave serial open failed device={} baud_rate={} err={}",
                            device, baud_rate, err
                        ));
                    });
                    log::warn!(
                        "failed to open Weave serial device={} baud_rate={} err={}",
                        device,
                        baud_rate,
                        err
                    );
                    tokio::select! {
                        _ = context.cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(RECONNECT_WAIT) => {}
                    }
                    continue;
                }
            };

            log::info!(
                "opened Weave serial device={} baud_rate={} iface={}",
                device,
                baud_rate,
                parent_iface
            );
            online.store(true, std::sync::atomic::Ordering::Release);
            update_weave_status(&runtime_status, |status| {
                status.link_state = WeaveLinkState::Discovering;
                status.last_error = None;
            });

            run_weave_stream_with_ifac(
                port,
                WeaveStreamOptions {
                    parent_iface,
                    device: device.clone(),
                    mtu,
                    iface_manager: iface_manager.clone(),
                    switch_identity: switch_identity.clone(),
                    runtime_status: runtime_status.clone(),
                },
                WeaveStreamControl {
                    cancel: context.cancel.clone(),
                    iface_stop: iface_stop.clone(),
                },
                rx_channel.clone(),
                tx_channel.clone(),
                management_frame_rx.clone(),
                IfacRuntime::from_parts(ifac_state.clone(), ifac_violations.clone()),
            )
            .await;
            online.store(false, std::sync::atomic::Ordering::Release);
        }

        online.store(false, std::sync::atomic::Ordering::Release);
        update_weave_status(&runtime_status, |status| {
            status.link_state = WeaveLinkState::Closed;
            status.wdcl_connected = false;
            status.last_error = None;
        });
        iface_stop.cancel();
    }
}

impl Interface for WeaveInterface {
    fn mtu() -> usize {
        DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

#[derive(Clone)]
pub(crate) struct WeaveStreamOptions {
    parent_iface: AddressHash,
    device: String,
    mtu: usize,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    switch_identity: PrivateIdentity,
    runtime_status: Arc<std::sync::Mutex<WeaveRuntimeStatus>>,
}

fn weave_management_channel() -> (WeaveManagementFrameSender, WeaveManagementFrameReceiver) {
    let (tx, rx) = tokio::sync::mpsc::channel(WEAVE_MANAGEMENT_CHANNEL_CAPACITY);
    (tx, Arc::new(tokio::sync::Mutex::new(rx)))
}

#[derive(Clone)]
pub struct WeaveManagementHandle {
    tx: WeaveManagementFrameSender,
    runtime_status: Arc<std::sync::Mutex<WeaveRuntimeStatus>>,
}

impl WeaveManagementHandle {
    pub fn try_set_remote_display(
        &self,
        remote_switch_id: Option<[u8; SWITCH_ID_LEN]>,
        enable: bool,
    ) -> io::Result<[u8; SWITCH_ID_LEN]> {
        let remote_switch_id = remote_switch_id
            .or_else(|| {
                self.runtime_status
                    .lock()
                    .expect("weave runtime status mutex poisoned")
                    .remote_switch_id
            })
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "remote_switch_id is required before Weave discovery has learned a switch",
                )
            })?;
        let frame = weave_wire_frame(&weave_remote_display_command_frame(remote_switch_id, enable));
        self.tx.try_send(frame).map_err(|err| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("queue Weave remote display command failed: {err}"),
            )
        })?;
        Ok(remote_switch_id)
    }
}

include!("weave_parts/status.rs");

include!("weave_parts/stream.rs");

include!("weave_parts/frame_processing.rs");

fn accept_discovery_response(
    local_identity: &PrivateIdentity,
    frame: &[u8],
) -> Option<[u8; SWITCH_ID_LEN]> {
    let local_switch_id = switch_id_for_identity(local_identity);
    let expected_len = SWITCH_ID_LEN + 1 + WEAVE_PUBKEY_SIZE + WEAVE_SIGNATURE_LEN;
    if frame.len() != expected_len || frame[..SWITCH_ID_LEN] != local_switch_id {
        return None;
    }
    let remote_pub = &frame[SWITCH_ID_LEN + 1..SWITCH_ID_LEN + 1 + WEAVE_PUBKEY_SIZE];
    let signature = Signature::from_slice(&frame[SWITCH_ID_LEN + 1 + WEAVE_PUBKEY_SIZE..]).ok()?;
    let remote = Identity::try_new_from_slices(remote_pub, remote_pub).ok()?;
    remote.verify(&local_switch_id, &signature).ok()?;
    let mut remote_switch_id = [0_u8; SWITCH_ID_LEN];
    remote_switch_id.copy_from_slice(&remote_pub[WEAVE_PUBKEY_SIZE - SWITCH_ID_LEN..]);
    Some(remote_switch_id)
}

fn weave_handshake_frame(
    local_identity: &PrivateIdentity,
    remote_switch_id: [u8; SWITCH_ID_LEN],
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(WEAVE_PUBKEY_SIZE + WEAVE_SIGNATURE_LEN);
    payload.extend_from_slice(local_identity.as_identity().verifying_key_bytes());
    payload.extend_from_slice(&local_identity.sign(&remote_switch_id).to_bytes());
    weave_wdcl_frame(remote_switch_id, WDCL_T_CONNECT, &payload)
}

fn weave_endpoint_command_frame(
    remote_switch_id: [u8; SWITCH_ID_LEN],
    endpoint: [u8; ENDPOINT_ID_LEN],
    payload: &[u8],
) -> Vec<u8> {
    let mut command = Vec::with_capacity(2 + ENDPOINT_ID_LEN + payload.len());
    command.extend_from_slice(&WDCL_CMD_ENDPOINT_PKT.to_be_bytes());
    command.extend_from_slice(&endpoint);
    command.extend_from_slice(payload);
    weave_wdcl_frame(remote_switch_id, WDCL_T_CMD, &command)
}

pub fn weave_remote_display_command_frame(remote_switch_id: [u8; 4], enable: bool) -> Vec<u8> {
    let command =
        [(WDCL_CMD_REMOTE_DISPLAY >> 8) as u8, WDCL_CMD_REMOTE_DISPLAY as u8, u8::from(enable)];
    weave_wdcl_frame(remote_switch_id, WDCL_T_CMD, &command)
}

fn weave_wdcl_frame(target: [u8; SWITCH_ID_LEN], packet_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(SWITCH_ID_LEN + 1 + payload.len());
    frame.extend_from_slice(&target);
    frame.push(packet_type);
    frame.extend_from_slice(payload);
    frame
}

fn weave_wire_frame(payload: &[u8]) -> Vec<u8> {
    let mut buffer = vec![0_u8; payload.len().saturating_mul(2).saturating_add(2)];
    let mut output = OutputBuffer::new(&mut buffer[..]);
    let len = Hdlc::encode(payload, &mut output).expect("weave HDLC buffer sized for worst case");
    buffer.truncate(len);
    buffer
}

fn switch_id_for_identity(identity: &PrivateIdentity) -> [u8; SWITCH_ID_LEN] {
    let verifying = identity.as_identity().verifying_key_bytes();
    let mut switch_id = [0_u8; SWITCH_ID_LEN];
    switch_id.copy_from_slice(&verifying[WEAVE_PUBKEY_SIZE - SWITCH_ID_LEN..]);
    switch_id
}

fn switch_id_from_slice(value: &[u8]) -> [u8; SWITCH_ID_LEN] {
    let mut out = [0_u8; SWITCH_ID_LEN];
    out.copy_from_slice(&value[..SWITCH_ID_LEN]);
    out
}

#[cfg(test)]
#[path = "weave_parts/tests/core.rs"]
mod tests;
