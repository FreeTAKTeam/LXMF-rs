use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use ed25519_dalek::Signature;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::Instant;
use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};
use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::hash::AddressHash;
use crate::identity::{Identity, PrivateIdentity};
use crate::iface::hdlc::Hdlc;
use crate::packet::Packet;
use crate::serde::Serialize;

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
            update_weave_status(&runtime_status, |status| {
                status.link_state = WeaveLinkState::Discovering;
                status.last_error = None;
            });

            run_weave_stream(
                port,
                WeaveStreamOptions {
                    parent_iface,
                    device: device.clone(),
                    mtu,
                    iface_manager: iface_manager.clone(),
                    switch_identity: switch_identity.clone(),
                    runtime_status: runtime_status.clone(),
                },
                context.cancel.clone(),
                iface_stop.clone(),
                rx_channel.clone(),
                tx_channel.clone(),
                management_frame_rx.clone(),
            )
            .await;
        }

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

#[derive(Debug, Default)]
struct WeaveRuntimeState {
    remote_switch_id: Option<[u8; SWITCH_ID_LEN]>,
    local_endpoint_id: Option<[u8; ENDPOINT_ID_LEN]>,
    endpoints: BTreeMap<[u8; ENDPOINT_ID_LEN], WeaveEndpointState>,
    addresses: BTreeMap<AddressHash, [u8; ENDPOINT_ID_LEN]>,
    wdcl_connected: bool,
}

#[derive(Debug)]
struct WeaveEndpointState {
    iface: AddressHash,
    last_seen: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaveLinkState {
    Configured,
    Discovering,
    Connected,
    Reconnecting,
    Closed,
}

impl WeaveLinkState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Discovering => "discovering",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
            Self::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaveEndpointRuntimeStatus {
    endpoint_id: [u8; ENDPOINT_ID_LEN],
    iface: AddressHash,
    alive_events: u64,
    via_events: u64,
    packets_rx: u64,
}

impl WeaveEndpointRuntimeStatus {
    fn new(endpoint_id: [u8; ENDPOINT_ID_LEN], iface: AddressHash) -> Self {
        Self { endpoint_id, iface, alive_events: 0, via_events: 0, packets_rx: 0 }
    }

    fn to_json(&self) -> serde_json::Value {
        let mut entry = serde_json::Map::new();
        entry.insert(
            "endpoint_id".to_string(),
            serde_json::Value::String(hex_bytes(&self.endpoint_id)),
        );
        entry.insert("iface".to_string(), serde_json::Value::String(self.iface.to_string()));
        entry.insert(
            "alive_events".to_string(),
            serde_json::Value::Number(self.alive_events.into()),
        );
        entry.insert("via_events".to_string(), serde_json::Value::Number(self.via_events.into()));
        entry.insert("packets_rx".to_string(), serde_json::Value::Number(self.packets_rx.into()));
        serde_json::Value::Object(entry)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaveRuntimeStatus {
    pub device: String,
    pub baud_rate: u32,
    pub mtu: usize,
    pub local_switch_id: [u8; SWITCH_ID_LEN],
    pub remote_switch_id: Option<[u8; SWITCH_ID_LEN]>,
    pub local_endpoint_id: Option<[u8; ENDPOINT_ID_LEN]>,
    pub link_state: WeaveLinkState,
    pub wdcl_connected: bool,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub frames_rx: u64,
    pub frames_tx: u64,
    pub invalid_frames: u64,
    pub log_events: BTreeMap<u16, u64>,
    pub endpoints: BTreeMap<[u8; ENDPOINT_ID_LEN], WeaveEndpointRuntimeStatus>,
    pub display: Option<WeaveDisplayStatus>,
    pub device_stats: WeaveDeviceStats,
    pub last_log_event: Option<u16>,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct WeaveRuntimeStatusHandle {
    inner: Arc<std::sync::Mutex<WeaveRuntimeStatus>>,
}

impl WeaveRuntimeStatusHandle {
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().expect("weave runtime status mutex poisoned").to_json()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaveDisplayStatus {
    pub color_format: u8,
    pub width: u16,
    pub height: u16,
    pub total_size: usize,
    pub received_size: usize,
    pub complete: bool,
    buffer: Vec<u8>,
    received: Vec<bool>,
}

impl WeaveDisplayStatus {
    fn new(color_format: u8, total_size: usize) -> Self {
        Self {
            color_format,
            width: 128,
            height: 64,
            total_size,
            received_size: 0,
            complete: false,
            buffer: vec![0; total_size],
            received: vec![false; total_size],
        }
    }

    fn apply_chunk(&mut self, offset: usize, data: &[u8]) {
        if offset >= self.buffer.len() {
            return;
        }
        let end = offset.saturating_add(data.len()).min(self.buffer.len());
        let len = end.saturating_sub(offset);
        self.buffer[offset..end].copy_from_slice(&data[..len]);
        for received in &mut self.received[offset..end] {
            if !*received {
                *received = true;
                self.received_size = self.received_size.saturating_add(1);
            }
        }
        self.complete = self.received_size >= self.total_size;
    }

    fn to_json(&self) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        root.insert(
            "color_format".to_string(),
            serde_json::Value::Number(self.color_format.into()),
        );
        root.insert("width".to_string(), serde_json::Value::Number(self.width.into()));
        root.insert("height".to_string(), serde_json::Value::Number(self.height.into()));
        root.insert(
            "total_size".to_string(),
            serde_json::Value::Number((self.total_size as u64).into()),
        );
        root.insert(
            "received_size".to_string(),
            serde_json::Value::Number((self.received_size as u64).into()),
        );
        root.insert("complete".to_string(), serde_json::Value::Bool(self.complete));
        if self.complete {
            root.insert(
                "buffer_hex".to_string(),
                serde_json::Value::String(hex_bytes(&self.buffer)),
            );
        }
        serde_json::Value::Object(root)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WeaveDeviceStats {
    pub cpu_load: Option<u8>,
    pub memory_free: Option<u32>,
    pub memory_total: Option<u32>,
    pub memory_used: Option<u32>,
    pub memory_used_percent_bp: Option<u32>,
    pub task_cpu: BTreeMap<String, WeaveTaskCpuStatus>,
}

impl WeaveDeviceStats {
    fn has_values(&self) -> bool {
        self.cpu_load.is_some() || self.memory_total.is_some() || !self.task_cpu.is_empty()
    }

    fn mark_cpu(&mut self, cpu_load: u8) {
        self.cpu_load = Some(cpu_load);
    }

    fn mark_task_cpu(&mut self, task: String, cpu_load: u8) {
        self.task_cpu
            .entry(task)
            .and_modify(|status| {
                status.cpu_load = cpu_load;
                status.samples = status.samples.saturating_add(1);
            })
            .or_insert(WeaveTaskCpuStatus { cpu_load, samples: 1 });
    }

    fn mark_memory(&mut self, memory_free: u32, memory_total: u32) {
        self.memory_free = Some(memory_free);
        self.memory_total = Some(memory_total);
        let memory_used = memory_total.saturating_sub(memory_free);
        self.memory_used = Some(memory_used);
        self.memory_used_percent_bp = (memory_total > 0)
            .then_some(((u64::from(memory_used) * 10_000) / u64::from(memory_total)) as u32);
    }

    fn to_json(&self) -> serde_json::Value {
        if !self.has_values() {
            return serde_json::Value::Null;
        }
        let mut root = serde_json::Map::new();
        root.insert(
            "cpu_load".to_string(),
            self.cpu_load
                .map(|value| serde_json::Value::Number(value.into()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "memory_free".to_string(),
            self.memory_free
                .map(|value| serde_json::Value::Number(value.into()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "memory_total".to_string(),
            self.memory_total
                .map(|value| serde_json::Value::Number(value.into()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "memory_used".to_string(),
            self.memory_used
                .map(|value| serde_json::Value::Number(value.into()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "memory_used_percent_bp".to_string(),
            self.memory_used_percent_bp
                .map(|value| serde_json::Value::Number(value.into()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "task_cpu".to_string(),
            serde_json::Value::Object(
                self.task_cpu
                    .iter()
                    .map(|(task, status)| (task.clone(), status.to_json()))
                    .collect(),
            ),
        );
        serde_json::Value::Object(root)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaveTaskCpuStatus {
    pub cpu_load: u8,
    pub samples: u64,
}

impl WeaveTaskCpuStatus {
    fn to_json(&self) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        root.insert("cpu_load".to_string(), serde_json::Value::Number(self.cpu_load.into()));
        root.insert("samples".to_string(), serde_json::Value::Number(self.samples.into()));
        serde_json::Value::Object(root)
    }
}

impl WeaveRuntimeStatus {
    #[must_use]
    pub fn new(
        device: String,
        baud_rate: u32,
        mtu: usize,
        local_switch_id: [u8; SWITCH_ID_LEN],
    ) -> Self {
        Self {
            device,
            baud_rate,
            mtu,
            local_switch_id,
            remote_switch_id: None,
            local_endpoint_id: None,
            link_state: WeaveLinkState::Configured,
            wdcl_connected: false,
            bytes_rx: 0,
            bytes_tx: 0,
            frames_rx: 0,
            frames_tx: 0,
            invalid_frames: 0,
            log_events: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            display: None,
            device_stats: WeaveDeviceStats::default(),
            last_log_event: None,
            last_error: None,
        }
    }

    fn mark_reconnecting(&mut self, error: String) {
        self.link_state = WeaveLinkState::Reconnecting;
        self.wdcl_connected = false;
        self.last_error = Some(error);
    }

    fn mark_endpoint(&mut self, endpoint_id: [u8; ENDPOINT_ID_LEN], iface: AddressHash) {
        self.endpoints
            .entry(endpoint_id)
            .or_insert_with(|| WeaveEndpointRuntimeStatus::new(endpoint_id, iface))
            .iface = iface;
    }

    fn mark_endpoint_alive(&mut self, endpoint_id: [u8; ENDPOINT_ID_LEN], iface: AddressHash) {
        self.mark_endpoint(endpoint_id, iface);
        if let Some(endpoint) = self.endpoints.get_mut(&endpoint_id) {
            endpoint.alive_events = endpoint.alive_events.saturating_add(1);
        }
    }

    fn mark_endpoint_via(&mut self, endpoint_id: [u8; ENDPOINT_ID_LEN], iface: AddressHash) {
        self.mark_endpoint(endpoint_id, iface);
        if let Some(endpoint) = self.endpoints.get_mut(&endpoint_id) {
            endpoint.via_events = endpoint.via_events.saturating_add(1);
        }
    }

    fn mark_endpoint_packet_rx(&mut self, endpoint_id: [u8; ENDPOINT_ID_LEN], iface: AddressHash) {
        self.mark_endpoint(endpoint_id, iface);
        if let Some(endpoint) = self.endpoints.get_mut(&endpoint_id) {
            endpoint.packets_rx = endpoint.packets_rx.saturating_add(1);
        }
    }

    fn remove_endpoint(&mut self, endpoint_id: &[u8; ENDPOINT_ID_LEN]) {
        self.endpoints.remove(endpoint_id);
    }

    fn clear_endpoints(&mut self) {
        self.endpoints.clear();
    }

    fn mark_display_chunk(
        &mut self,
        color_format: u8,
        offset: usize,
        total_size: usize,
        data: &[u8],
    ) {
        if total_size == 0 {
            return;
        }
        let reset_display = self.display.as_ref().is_none_or(|display| {
            display.total_size != total_size || display.color_format != color_format
        });
        if reset_display {
            self.display = Some(WeaveDisplayStatus::new(color_format, total_size));
        }
        if let Some(display) = self.display.as_mut() {
            display.apply_chunk(offset, data);
        }
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        root.insert("device".to_string(), serde_json::Value::String(self.device.clone()));
        root.insert("baud_rate".to_string(), serde_json::Value::Number(self.baud_rate.into()));
        root.insert("mtu".to_string(), serde_json::Value::Number((self.mtu as u64).into()));
        root.insert(
            "link_state".to_string(),
            serde_json::Value::String(self.link_state.as_str().to_string()),
        );
        root.insert("wdcl_connected".to_string(), serde_json::Value::Bool(self.wdcl_connected));
        root.insert(
            "local_switch_id".to_string(),
            serde_json::Value::String(hex_bytes(&self.local_switch_id)),
        );
        root.insert(
            "remote_switch_id".to_string(),
            self.remote_switch_id
                .as_ref()
                .map(|value| serde_json::Value::String(hex_bytes(value)))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "local_endpoint_id".to_string(),
            self.local_endpoint_id
                .as_ref()
                .map(|value| serde_json::Value::String(hex_bytes(value)))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "endpoint_count".to_string(),
            serde_json::Value::Number((self.endpoints.len() as u64).into()),
        );
        root.insert("bytes_rx".to_string(), serde_json::Value::Number(self.bytes_rx.into()));
        root.insert("bytes_tx".to_string(), serde_json::Value::Number(self.bytes_tx.into()));
        root.insert("frames_rx".to_string(), serde_json::Value::Number(self.frames_rx.into()));
        root.insert("frames_tx".to_string(), serde_json::Value::Number(self.frames_tx.into()));
        root.insert(
            "invalid_frames".to_string(),
            serde_json::Value::Number(self.invalid_frames.into()),
        );
        root.insert(
            "last_log_event".to_string(),
            self.last_log_event
                .map(|event| serde_json::Value::String(format!("0x{event:04x}")))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "last_error".to_string(),
            self.last_error
                .as_ref()
                .map(|err| serde_json::Value::String(err.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "log_events".to_string(),
            serde_json::Value::Object(
                self.log_events
                    .iter()
                    .map(|(event, count)| {
                        (format!("0x{event:04x}"), serde_json::Value::Number((*count).into()))
                    })
                    .collect(),
            ),
        );
        root.insert(
            "endpoints".to_string(),
            serde_json::Value::Array(
                self.endpoints.values().map(WeaveEndpointRuntimeStatus::to_json).collect(),
            ),
        );
        root.insert(
            "display".to_string(),
            self.display
                .as_ref()
                .map(WeaveDisplayStatus::to_json)
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert("device_stats".to_string(), self.device_stats.to_json());
        serde_json::Value::Object(root)
    }
}

fn update_weave_status(
    runtime_status: &Arc<std::sync::Mutex<WeaveRuntimeStatus>>,
    update: impl FnOnce(&mut WeaveRuntimeStatus),
) {
    let mut guard = runtime_status.lock().expect("weave runtime status mutex poisoned");
    update(&mut guard);
}

fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{:02x}", *byte);
    }
    out
}

pub(crate) async fn run_weave_stream<IO>(
    mut stream: IO,
    options: WeaveStreamOptions,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
    management_frame_rx: WeaveManagementFrameReceiver,
) where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let switch_id = switch_id_for_identity(&options.switch_identity);
    let discover = weave_wire_frame(&weave_wdcl_frame(WDCL_BROADCAST, WDCL_T_DISCOVER, &switch_id));
    if let Err(err) = stream.write_all(&discover).await {
        update_weave_status(&options.runtime_status, |status| {
            status.mark_reconnecting(format!(
                "weave discovery write failed iface={} device={} err={}",
                options.parent_iface, options.device, err
            ));
        });
        log::warn!(
            "Weave discovery write error iface={} device={} err={}",
            options.parent_iface,
            options.device,
            err
        );
        return;
    }
    let _ = stream.flush().await;
    update_weave_status(&options.runtime_status, |status| {
        status.link_state = WeaveLinkState::Discovering;
        status.bytes_tx = status.bytes_tx.saturating_add(discover.len() as u64);
        status.frames_tx = status.frames_tx.saturating_add(1);
    });

    let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
    let mut frame_buffer = Vec::<u8>::with_capacity(options.mtu * 4);
    let mut hdlc_rx_buffer = vec![0_u8; options.mtu.max(DEFAULT_MTU) + 128];
    let mut read_buffer = vec![0_u8; READ_CAPACITY.max(options.mtu)];
    let mut tx_buffer = vec![0_u8; options.mtu];
    let mut cleanup_interval = tokio::time::interval(WEAVE_ENDPOINT_CLEANUP_INTERVAL);
    let mut stopped_by_control = false;

    'stream_loop: loop {
        let mut tx_channel = tx_channel.lock().await;
        let mut management_frame_rx = management_frame_rx.lock().await;
        tokio::select! {
            _ = cancel.cancelled() => {
                stopped_by_control = true;
                break;
            }
            _ = iface_stop.cancelled() => {
                stopped_by_control = true;
                break;
            }
            _ = cleanup_interval.tick() => {
                gc_weave_endpoints(&options, state.clone(), Instant::now()).await;
            }
            result = stream.read(&mut read_buffer[..]) => {
                match result {
                    Ok(0) => {
                        update_weave_status(&options.runtime_status, |status| {
                            status.link_state = WeaveLinkState::Closed;
                            status.wdcl_connected = false;
                        });
                        break;
                    }
                    Ok(n) => {
                        update_weave_status(&options.runtime_status, |status| {
                            status.bytes_rx = status.bytes_rx.saturating_add(n as u64);
                        });
                        frame_buffer.extend_from_slice(&read_buffer[..n]);
                        while let Some((start, end)) = Hdlc::find(&frame_buffer) {
                            let frame = &frame_buffer[start..=end];
                            let mut output = OutputBuffer::new(&mut hdlc_rx_buffer[..]);
                            if Hdlc::decode(frame, &mut output).is_ok() {
                                update_weave_status(&options.runtime_status, |status| {
                                    status.frames_rx = status.frames_rx.saturating_add(1);
                                });
                                if !process_weave_frame(
                                    output.as_slice(),
                                    &options,
                                    state.clone(),
                                    &rx_channel,
                                    &mut stream,
                                )
                                .await
                                {
                                    break 'stream_loop;
                                }
                            } else {
                                update_weave_status(&options.runtime_status, |status| {
                                    status.invalid_frames = status.invalid_frames.saturating_add(1);
                                });
                            }
                            frame_buffer.drain(..=end);
                        }
                        if frame_buffer.len() > options.mtu * 64 {
                            frame_buffer.clear();
                        }
                    }
                    Err(err) => {
                        update_weave_status(&options.runtime_status, |status| {
                            status.mark_reconnecting(format!(
                                "weave read failed iface={} device={} err={}",
                                options.parent_iface, options.device, err
                            ));
                        });
                        log::warn!(
                            "Weave read error iface={} device={} err={}",
                            options.parent_iface,
                            options.device,
                            err
                        );
                        break;
                    }
                }
            }
            Some(message) = tx_channel.recv() => {
                let Some(frames) = weave_tx_frames(&message, &options, state.clone(), &mut tx_buffer).await else {
                    continue;
                };
                for frame in frames {
                    if let Err(err) = stream.write_all(&frame).await {
                        update_weave_status(&options.runtime_status, |status| {
                            status.mark_reconnecting(format!(
                                "weave write failed iface={} device={} err={}",
                                options.parent_iface, options.device, err
                            ));
                        });
                        log::warn!(
                            "Weave write error iface={} device={} err={}",
                            options.parent_iface,
                            options.device,
                            err
                        );
                        break 'stream_loop;
                    }
                    let _ = stream.flush().await;
                    update_weave_status(&options.runtime_status, |status| {
                        status.bytes_tx = status.bytes_tx.saturating_add(frame.len() as u64);
                        status.frames_tx = status.frames_tx.saturating_add(1);
                    });
                }
            }
            Some(frame) = management_frame_rx.recv() => {
                if let Err(err) = stream.write_all(&frame).await {
                    update_weave_status(&options.runtime_status, |status| {
                        status.mark_reconnecting(format!(
                            "weave management write failed iface={} device={} err={}",
                            options.parent_iface, options.device, err
                        ));
                    });
                    log::warn!(
                        "Weave management write error iface={} device={} err={}",
                        options.parent_iface,
                        options.device,
                        err
                    );
                    break 'stream_loop;
                }
                let _ = stream.flush().await;
                update_weave_status(&options.runtime_status, |status| {
                    status.bytes_tx = status.bytes_tx.saturating_add(frame.len() as u64);
                    status.frames_tx = status.frames_tx.saturating_add(1);
                });
            }
        }
    }
    cleanup_weave_endpoints(&options, state).await;
    if stopped_by_control {
        update_weave_status(&options.runtime_status, |status| {
            status.link_state = WeaveLinkState::Closed;
            status.wdcl_connected = false;
            status.last_error = None;
        });
    }
}

async fn process_weave_frame<IO>(
    frame: &[u8],
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
    rx_channel: &tokio::sync::mpsc::Sender<RxMessage>,
    stream: &mut IO,
) -> bool
where
    IO: AsyncWrite + Unpin,
{
    if frame.len() <= SWITCH_ID_LEN {
        return true;
    }
    let target = switch_id_from_slice(&frame[..SWITCH_ID_LEN]);
    let packet_type = frame[SWITCH_ID_LEN];
    let payload = &frame[SWITCH_ID_LEN + 1..];
    let local_switch_id = switch_id_for_identity(&options.switch_identity);

    match packet_type {
        WDCL_T_DISCOVER => {
            if target == local_switch_id {
                if let Some(remote_switch_id) =
                    accept_discovery_response(&options.switch_identity, frame)
                {
                    state.lock().await.remote_switch_id = Some(remote_switch_id);
                    let handshake =
                        weave_handshake_frame(&options.switch_identity, remote_switch_id);
                    update_weave_status(&options.runtime_status, |status| {
                        status.remote_switch_id = Some(remote_switch_id);
                        status.last_error = None;
                    });
                    let handshake = weave_wire_frame(&handshake);
                    match stream.write_all(&handshake).await {
                        Ok(()) => {
                            let _ = stream.flush().await;
                            update_weave_status(&options.runtime_status, |status| {
                                status.bytes_tx =
                                    status.bytes_tx.saturating_add(handshake.len() as u64);
                                status.frames_tx = status.frames_tx.saturating_add(1);
                            });
                        }
                        Err(err) => {
                            update_weave_status(&options.runtime_status, |status| {
                                status.mark_reconnecting(format!(
                                    "weave handshake write failed iface={} device={} err={}",
                                    options.parent_iface, options.device, err
                                ));
                            });
                            log::warn!(
                                "Weave handshake write error iface={} device={} err={}",
                                options.parent_iface,
                                options.device,
                                err
                            );
                            return false;
                        }
                    }
                }
            }
        }
        WDCL_T_ENDPOINT_PKT => {
            if target == local_switch_id && payload.len() > ENDPOINT_ID_LEN {
                let data_len = payload.len() - ENDPOINT_ID_LEN;
                let mut endpoint = [0_u8; ENDPOINT_ID_LEN];
                endpoint.copy_from_slice(&payload[data_len..]);
                let address = ensure_weave_endpoint(endpoint, options, state.clone()).await;
                if let Some(address) = address {
                    if let Ok(packet) =
                        Packet::deserialize(&mut InputBuffer::new(&payload[..data_len]))
                    {
                        update_weave_status(&options.runtime_status, |status| {
                            status.mark_endpoint_packet_rx(endpoint, address);
                        });
                        let _ = rx_channel
                            .send(RxMessage { address, packet, source: IfaceSource::None })
                            .await;
                    }
                }
            }
        }
        WDCL_T_LOG => {
            if target == local_switch_id {
                process_weave_log(payload, options, state).await;
            }
        }
        WDCL_T_DISP if target == local_switch_id => {
            process_weave_display(payload, options);
        }
        _ => {}
    }
    true
}

fn process_weave_display(payload: &[u8], options: &WeaveStreamOptions) {
    if payload.len() < 9 {
        return;
    }
    let color_format = payload[0];
    let offset = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]) as usize;
    let total_size = u32::from_be_bytes([payload[5], payload[6], payload[7], payload[8]]) as usize;
    let data = &payload[9..];
    update_weave_status(&options.runtime_status, |status| {
        status.mark_display_chunk(color_format, offset, total_size, data);
    });
}

async fn process_weave_log(
    payload: &[u8],
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
) {
    if payload.len() < 8 {
        return;
    }
    let event = u16::from_be_bytes([payload[6], payload[7]]);
    let data = &payload[8..];
    update_weave_status(&options.runtime_status, |status| {
        status.last_log_event = Some(event);
        *status.log_events.entry(event).or_insert(0) += 1;
    });
    match event {
        ET_PROTO_WDCL_CONNECTION => {
            state.lock().await.wdcl_connected = true;
            update_weave_status(&options.runtime_status, |status| {
                status.wdcl_connected = true;
                status.link_state = WeaveLinkState::Connected;
                status.last_error = None;
            });
        }
        ET_PROTO_WDCL_HOST_ENDPOINT if data.len() == ENDPOINT_ID_LEN => {
            let mut endpoint = [0_u8; ENDPOINT_ID_LEN];
            endpoint.copy_from_slice(data);
            state.lock().await.local_endpoint_id = Some(endpoint);
            update_weave_status(&options.runtime_status, |status| {
                status.local_endpoint_id = Some(endpoint);
            });
        }
        ET_PROTO_WEAVE_EP_ALIVE if data.len() == ENDPOINT_ID_LEN => {
            let mut endpoint = [0_u8; ENDPOINT_ID_LEN];
            endpoint.copy_from_slice(data);
            if let Some(address) = ensure_weave_endpoint(endpoint, options, state).await {
                update_weave_status(&options.runtime_status, |status| {
                    status.mark_endpoint_alive(endpoint, address);
                });
            }
        }
        ET_PROTO_WEAVE_EP_VIA if data.len() >= ENDPOINT_ID_LEN => {
            let mut endpoint = [0_u8; ENDPOINT_ID_LEN];
            endpoint.copy_from_slice(&data[..ENDPOINT_ID_LEN]);
            if let Some(address) = ensure_weave_endpoint(endpoint, options, state).await {
                update_weave_status(&options.runtime_status, |status| {
                    status.mark_endpoint_via(endpoint, address);
                });
            }
        }
        ET_STAT_CPU if !data.is_empty() => {
            update_weave_status(&options.runtime_status, |status| {
                status.device_stats.mark_cpu(data[0]);
            });
        }
        ET_STAT_TASK_CPU if data.len() >= 2 => {
            let task = String::from_utf8_lossy(&data[1..]).trim_end_matches('\0').to_string();
            if !task.is_empty() {
                update_weave_status(&options.runtime_status, |status| {
                    status.device_stats.mark_task_cpu(task, data[0]);
                });
            }
        }
        ET_STAT_MEMORY if data.len() >= 8 => {
            let memory_free = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            let memory_total = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            update_weave_status(&options.runtime_status, |status| {
                status.device_stats.mark_memory(memory_free, memory_total);
            });
        }
        _ => {}
    }
}

async fn weave_tx_frames(
    message: &TxMessage,
    _options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
    tx_buffer: &mut [u8],
) -> Option<Vec<Vec<u8>>> {
    let mut output = OutputBuffer::new(tx_buffer);
    message.packet.serialize(&mut output).ok()?;
    let payload = output.as_slice();
    let mut state = state.lock().await;
    let remote_switch_id = state.remote_switch_id?;
    let endpoints: Vec<[u8; ENDPOINT_ID_LEN]> = match message.tx_type {
        TxMessageType::Direct(address) => match state.addresses.get(&address).copied() {
            Some(endpoint) => {
                if let Some(entry) = state.endpoints.get_mut(&endpoint) {
                    entry.last_seen = Instant::now();
                }
                vec![endpoint]
            }
            None => Vec::new(),
        },
        TxMessageType::Broadcast(_) => {
            let endpoints: Vec<[u8; ENDPOINT_ID_LEN]> = state.endpoints.keys().copied().collect();
            let now = Instant::now();
            for endpoint in &endpoints {
                if let Some(entry) = state.endpoints.get_mut(endpoint) {
                    entry.last_seen = now;
                }
            }
            endpoints
        }
    };
    drop(state);

    let mut frames = Vec::new();
    for endpoint in endpoints {
        frames.push(weave_wire_frame(&weave_endpoint_command_frame(
            remote_switch_id,
            endpoint,
            payload,
        )));
    }
    (!frames.is_empty()).then_some(frames)
}

async fn ensure_weave_endpoint(
    endpoint: [u8; ENDPOINT_ID_LEN],
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
) -> Option<AddressHash> {
    {
        let mut state = state.lock().await;
        if let Some(entry) = state.endpoints.get_mut(&endpoint) {
            entry.last_seen = Instant::now();
            return Some(entry.iface);
        }
    }
    let mut manager = options.iface_manager.lock().await;
    let address =
        manager.register_virtual_iface(options.parent_iface, IfaceRole::VirtualUnicast)?;
    manager.set_outgoing(address, true);
    drop(manager);

    let mut state = state.lock().await;
    state
        .endpoints
        .insert(endpoint, WeaveEndpointState { iface: address, last_seen: Instant::now() });
    state.addresses.insert(address, endpoint);
    Some(address)
}

async fn gc_weave_endpoints(
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
    now: Instant,
) {
    let stale = {
        let mut state = state.lock().await;
        let stale: Vec<([u8; ENDPOINT_ID_LEN], AddressHash)> = state
            .endpoints
            .iter()
            .filter(|(_, endpoint)| {
                now.duration_since(endpoint.last_seen) >= WEAVE_ENDPOINT_IDLE_TIMEOUT
            })
            .map(|(endpoint_id, endpoint)| (*endpoint_id, endpoint.iface))
            .collect();
        for (endpoint_id, iface) in &stale {
            state.endpoints.remove(endpoint_id);
            state.addresses.remove(iface);
        }
        stale
    };

    if stale.is_empty() {
        return;
    }

    for (_, iface) in &stale {
        let _ = options.iface_manager.lock().await.stop_interface(*iface);
    }
    update_weave_status(&options.runtime_status, |status| {
        for (endpoint_id, _) in &stale {
            status.remove_endpoint(endpoint_id);
        }
    });
}

async fn cleanup_weave_endpoints(
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
) {
    let endpoints = {
        let mut state = state.lock().await;
        let endpoints: Vec<([u8; ENDPOINT_ID_LEN], AddressHash)> = state
            .endpoints
            .iter()
            .map(|(endpoint_id, endpoint)| (*endpoint_id, endpoint.iface))
            .collect();
        state.endpoints.clear();
        state.addresses.clear();
        endpoints
    };

    if endpoints.is_empty() {
        return;
    }

    for (_, iface) in &endpoints {
        let _ = options.iface_manager.lock().await.stop_interface(*iface);
    }
    update_weave_status(&options.runtime_status, WeaveRuntimeStatus::clear_endpoints);
}

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
    let remote = Identity::new_from_slices(remote_pub, remote_pub);
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
mod tests {
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
    use crate::packet::Packet;

    use super::*;

    fn packet_payload(packet: &Packet) -> Vec<u8> {
        let mut buffer = vec![0_u8; 512];
        let mut output = OutputBuffer::new(&mut buffer);
        packet.serialize(&mut output).expect("serialize packet");
        output.as_slice().to_vec()
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
    }

    impl FailingWeaveWriteStream {
        fn new(read_chunks: Vec<Vec<u8>>, successful_writes: usize) -> Self {
            Self { read_chunks: read_chunks.into(), successful_writes, write_attempts: 0 }
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

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
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
        let address = ensure_weave_endpoint(endpoint, &options, state.clone())
            .await
            .expect("register endpoint");
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
        let address = ensure_weave_endpoint(endpoint, &options, state.clone())
            .await
            .expect("register endpoint");
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
        let address = ensure_weave_endpoint(endpoint, &options, state.clone())
            .await
            .expect("register endpoint");
        {
            let mut state = state.lock().await;
            state.endpoints.get_mut(&endpoint).expect("endpoint state").last_seen =
                Instant::now() - WEAVE_ENDPOINT_IDLE_TIMEOUT - Duration::from_secs(1);
        }

        let refreshed_address = ensure_weave_endpoint(endpoint, &options, state.clone())
            .await
            .expect("refresh endpoint");
        gc_weave_endpoints(&options, state.clone(), Instant::now()).await;

        assert_eq!(refreshed_address, address);
        assert!(state.lock().await.endpoints.contains_key(&endpoint));
        assert_eq!(manager.lock().await.role(&address), Some(IfaceRole::VirtualUnicast));
    }

    #[tokio::test]
    async fn weave_tx_frames_refreshes_endpoint_activity() {
        let (options, manager, _parent) = test_options().await;
        let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
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

        let mut tx_buffer = vec![0_u8; DEFAULT_MTU];
        let frames = weave_tx_frames(
            &TxMessage {
                tx_type: TxMessageType::Direct(direct_address),
                packet: Packet::default(),
            },
            &options,
            state.clone(),
            &mut tx_buffer,
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
            &mut tx_buffer,
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
        let mut endpoint_payload = packet_payload(&Packet::default());
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
        runtime_status.lock().expect("weave runtime status").remote_switch_id =
            Some(learned_switch);
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
        let mut endpoint_payload = packet_payload(&Packet::default());
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
        let mut endpoint_payload = packet_payload(&Packet::default());
        endpoint_payload.extend_from_slice(&endpoint);
        let stream = FailingWeaveWriteStream::new(
            vec![
                weave_wire_frame(&weave_wdcl_frame(
                    local_switch,
                    WDCL_T_DISCOVER,
                    &discovery_payload,
                )),
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
        peer.write_all(&log_frame(local_switch, ET_STAT_MEMORY, &memory))
            .await
            .expect("memory stat");
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
        peer.write_all(&log_frame(off_target, ET_STAT_CPU, &[73]))
            .await
            .expect("off-target cpu stat");
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
}
