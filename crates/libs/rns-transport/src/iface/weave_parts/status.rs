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
