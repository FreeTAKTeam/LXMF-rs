use alloc::{collections::VecDeque, vec::Vec};

pub use constants::*;

pub use node::{
    BleNodeBackendConfig, BroadcastOptions, CaptureDefaults, EmbeddedNode, EventSubscription,
    NodeBackendConfig, NodeConfig, NodeError, NodeEvent, NodeEventKind, NodeLifecycleState,
    NodeLogLevel, NodeOperationKind, NodeOperationReceipt, NodeRunState, NodeStatus,
    NodeTransportMode, PollResult, SendOptions, TcpClientConfig, TcpServerConfig,
};

use rns_embedded_core::{
    lxmf_min::{decode_envelope, encode_envelope, MinimalEnvelope},
    packet::PacketFrame,
    replay::ReplayWindow,
    store::EmbeddedStore,
    transport::{EmbeddedTransport, LinkState},
    EmbeddedError, EmbeddedResult,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RuntimeConfig {
    pub store_identity: [u8; 32],
    pub lxmf_address: [u8; 16],
    pub node_mode: NodeTransportMode,
    pub announce_interval_ms: u64,
    pub max_outbound_queue: usize,
    pub max_events: usize,
    pub capture_defaults: CaptureDefaults,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            store_identity: [0x11; 32],
            lxmf_address: [0x22; 16],
            node_mode: NodeTransportMode::BleOnly,
            announce_interval_ms: DEFAULT_ANNOUNCE_INTERVAL_MS,
            max_outbound_queue: 8,
            max_events: 32,
            capture_defaults: CaptureDefaults::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct RuntimeStats {
    pub announces_queued: u32,
    pub outbound_sent: u32,
    pub outbound_deferred: u32,
    pub inbound_accepted: u32,
    pub inbound_rejected: u32,
    pub announces_received: u32,
    pub lxmf_messages_received: u32,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RuntimeEvent {
    Bootstrapped {
        replay_floor: u64,
    },
    AnnounceQueued {
        sequence: u32,
    },
    MessageQueued {
        sequence: u32,
        bytes: usize,
    },
    FrameSent {
        kind: u8,
        sequence: u32,
        bytes: usize,
    },
    FrameDeferred {
        kind: u8,
        sequence: u32,
        error: EmbeddedError,
    },
    FrameReceived {
        kind: u8,
        sequence: u32,
        bytes: usize,
    },
    AnnounceReceived {
        sequence: u32,
        bytes: usize,
    },
    LxmfMessageReceived {
        sequence: u32,
        source: [u8; 16],
        destination: [u8; 16],
        body_bytes: usize,
    },
    FrameRejected {
        kind: u8,
        sequence: u32,
        error: EmbeddedError,
    },
    LifecycleChanged {
        from: NodeLifecycleState,
        to: NodeLifecycleState,
    },
}

pub struct EmbeddedNodeRuntime {
    config: RuntimeConfig,
    replay_floor: u64,
    replay_window: ReplayWindow,
    next_sequence: u32,
    outbound: VecDeque<PacketFrame>,
    events: VecDeque<RuntimeEvent>,
    last_announce_at_ms: Option<u64>,
    bootstrapped: bool,
    stats: RuntimeStats,
    network_provisioned: bool,
    ble_recovery_active: bool,
    lifecycle_state: NodeLifecycleState,
}

impl EmbeddedNodeRuntime {
    pub fn new(config: RuntimeConfig) -> EmbeddedResult<Self> {
        if config.store_identity == [0; 32] || config.lxmf_address == [0; 16] {
            return Err(EmbeddedError::InvalidInput);
        }
        if config.max_outbound_queue == 0 || config.max_events == 0 {
            return Err(EmbeddedError::InvalidArgument);
        }
        Ok(Self {
            config,
            replay_floor: 0,
            replay_window: ReplayWindow::new(),
            next_sequence: 1,
            outbound: VecDeque::new(),
            events: VecDeque::new(),
            last_announce_at_ms: None,
            bootstrapped: false,
            stats: RuntimeStats {
                announces_queued: 0,
                outbound_sent: 0,
                outbound_deferred: 0,
                inbound_accepted: 0,
                inbound_rejected: 0,
                announces_received: 0,
                lxmf_messages_received: 0,
            },
            network_provisioned: false,
            ble_recovery_active: false,
            lifecycle_state: NodeLifecycleState::Boot,
        })
    }

    pub fn bootstrap<S: EmbeddedStore>(&mut self, store: &S) -> EmbeddedResult<()> {
        let replay_floor = store.load_replay_floor(&self.config.store_identity)?;
        self.replay_floor = replay_floor;
        self.bootstrapped = true;
        self.push_event(RuntimeEvent::Bootstrapped { replay_floor });
        Ok(())
    }

    pub fn tick<T: EmbeddedTransport, S: EmbeddedStore>(
        &mut self,
        now_ms: u64,
        transport: &mut T,
        store: &mut S,
    ) -> EmbeddedResult<()> {
        if !self.bootstrapped {
            self.bootstrap(store)?;
        }

        self.update_lifecycle(transport.link_state());

        if transport.link_state() == LinkState::Up
            && self.announce_due(now_ms)
            && !self.has_queued_kind(FRAME_KIND_ANNOUNCE)
        {
            let sequence = self.queue_announce()?;
            self.last_announce_at_ms = Some(now_ms);
            self.stats.announces_queued = self.stats.announces_queued.saturating_add(1);
            self.push_event(RuntimeEvent::AnnounceQueued { sequence });
        }

        self.poll_inbound(transport, store)?;
        self.flush_outbound(transport)?;
        Ok(())
    }

    pub fn queue_message(&mut self, destination: [u8; 16], body: &[u8]) -> EmbeddedResult<u32> {
        let sequence = self.peek_next_sequence();
        let envelope = MinimalEnvelope {
            source: self.config.lxmf_address,
            destination,
            sequence: u64::from(sequence),
            body: body.to_vec(),
        };
        let payload = encode_envelope(&envelope)?;
        let frame = PacketFrame::new(FRAME_KIND_LXMF_MESSAGE, sequence, payload)?;
        self.enqueue_frame(frame)?;
        self.push_event(RuntimeEvent::MessageQueued { sequence, bytes: body.len() });
        Ok(sequence)
    }

    pub fn pending_outbound_len(&self) -> usize {
        self.outbound.len()
    }

    pub fn config(&self) -> RuntimeConfig {
        self.config
    }

    pub fn lifecycle_state(&self) -> NodeLifecycleState {
        self.lifecycle_state
    }

    pub fn set_network_provisioned(&mut self, provisioned: bool) {
        self.network_provisioned = provisioned;
    }

    pub fn set_ble_recovery_active(&mut self, active: bool) {
        self.ble_recovery_active = active;
    }

    pub fn stats(&self) -> RuntimeStats {
        self.stats
    }

    pub fn drain_events(&mut self) -> Vec<RuntimeEvent> {
        self.events.drain(..).collect()
    }

    fn poll_inbound<T: EmbeddedTransport, S: EmbeddedStore>(
        &mut self,
        transport: &mut T,
        store: &mut S,
    ) -> EmbeddedResult<()> {
        for _ in 0..8 {
            let Some(frame) = transport.poll_frame()? else {
                break;
            };
            let sequence = u64::from(frame.sequence);
            if sequence <= self.replay_floor || !self.replay_window.accept(sequence) {
                self.stats.inbound_rejected = self.stats.inbound_rejected.saturating_add(1);
                self.push_event(RuntimeEvent::FrameRejected {
                    kind: frame.kind,
                    sequence: frame.sequence,
                    error: EmbeddedError::ReplayRejected,
                });
                continue;
            }

            self.replay_floor = sequence;
            store.save_replay_floor(&self.config.store_identity, self.replay_floor)?;
            self.stats.inbound_accepted = self.stats.inbound_accepted.saturating_add(1);
            self.push_event(RuntimeEvent::FrameReceived {
                kind: frame.kind,
                sequence: frame.sequence,
                bytes: frame.payload.len(),
            });
            self.handle_inbound_frame(frame)?;
        }
        Ok(())
    }

    fn handle_inbound_frame(&mut self, frame: PacketFrame) -> EmbeddedResult<()> {
        match frame.kind {
            FRAME_KIND_ANNOUNCE => {
                self.stats.announces_received = self.stats.announces_received.saturating_add(1);
                self.push_event(RuntimeEvent::AnnounceReceived {
                    sequence: frame.sequence,
                    bytes: frame.payload.len(),
                });
            }
            FRAME_KIND_LXMF_MESSAGE => {
                let envelope = decode_envelope(&frame.payload)?;
                self.stats.lxmf_messages_received =
                    self.stats.lxmf_messages_received.saturating_add(1);
                self.push_event(RuntimeEvent::LxmfMessageReceived {
                    sequence: frame.sequence,
                    source: envelope.source,
                    destination: envelope.destination,
                    body_bytes: envelope.body.len(),
                });
                if envelope.destination == self.config.lxmf_address {
                    let mut response_body = b"pong:".to_vec();
                    response_body.extend_from_slice(&envelope.body);
                    self.queue_message(envelope.source, &response_body)?;
                }
            }
            FRAME_KIND_TEST_PING => {
                let mut payload = b"pong:".to_vec();
                payload.extend_from_slice(&frame.payload);
                let sequence = self.peek_next_sequence();
                let response = PacketFrame::new(FRAME_KIND_TEST_PONG, sequence, payload)?;
                self.enqueue_frame(response)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn flush_outbound<T: EmbeddedTransport>(&mut self, transport: &mut T) -> EmbeddedResult<()> {
        if transport.link_state() != LinkState::Up {
            return Ok(());
        }

        while let Some(frame) = self.outbound.front().cloned() {
            match transport.send_frame(&frame) {
                Ok(()) => {
                    self.outbound.pop_front();
                    self.stats.outbound_sent = self.stats.outbound_sent.saturating_add(1);
                    self.push_event(RuntimeEvent::FrameSent {
                        kind: frame.kind,
                        sequence: frame.sequence,
                        bytes: frame.payload.len(),
                    });
                }
                Err(error @ (EmbeddedError::Backpressure | EmbeddedError::Disconnected)) => {
                    self.stats.outbound_deferred = self.stats.outbound_deferred.saturating_add(1);
                    self.push_event(RuntimeEvent::FrameDeferred {
                        kind: frame.kind,
                        sequence: frame.sequence,
                        error,
                    });
                    break;
                }
                Err(error) => {
                    self.outbound.pop_front();
                    self.stats.outbound_deferred = self.stats.outbound_deferred.saturating_add(1);
                    self.push_event(RuntimeEvent::FrameDeferred {
                        kind: frame.kind,
                        sequence: frame.sequence,
                        error,
                    });
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn queue_announce(&mut self) -> EmbeddedResult<u32> {
        let sequence = self.peek_next_sequence();
        let frame =
            PacketFrame::new(FRAME_KIND_ANNOUNCE, sequence, self.config.store_identity.to_vec())?;
        self.enqueue_frame(frame)?;
        Ok(sequence)
    }

    fn enqueue_frame(&mut self, frame: PacketFrame) -> EmbeddedResult<()> {
        if self.outbound.len() >= self.config.max_outbound_queue {
            return Err(EmbeddedError::Backpressure);
        }
        self.outbound.push_back(frame);
        self.next_sequence = self.next_sequence.saturating_add(1);
        Ok(())
    }

    fn has_queued_kind(&self, kind: u8) -> bool {
        self.outbound.iter().any(|frame| frame.kind == kind)
    }

    fn announce_due(&self, now_ms: u64) -> bool {
        match self.last_announce_at_ms {
            None => true,
            Some(last) => now_ms.saturating_sub(last) >= self.config.announce_interval_ms,
        }
    }

    fn update_lifecycle(&mut self, link_state: LinkState) {
        let next = if !self.bootstrapped {
            NodeLifecycleState::Boot
        } else if self.ble_recovery_active {
            NodeLifecycleState::BleRecovery
        } else {
            match self.config.node_mode {
                NodeTransportMode::BleOnly => NodeLifecycleState::Unprovisioned,
                NodeTransportMode::TcpClient | NodeTransportMode::TcpServer => {
                    if !self.network_provisioned {
                        NodeLifecycleState::Unprovisioned
                    } else {
                        match link_state {
                            LinkState::Up => NodeLifecycleState::TcpOnline,
                            LinkState::Connecting => NodeLifecycleState::FailureReconnect,
                            LinkState::Down => NodeLifecycleState::ProvisionedOffline,
                        }
                    }
                }
            }
        };

        if self.lifecycle_state != next {
            let from = self.lifecycle_state;
            self.lifecycle_state = next;
            self.push_event(RuntimeEvent::LifecycleChanged { from, to: next });
        }
    }

    fn peek_next_sequence(&self) -> u32 {
        self.next_sequence
    }

    fn push_event(&mut self, event: RuntimeEvent) {
        if self.events.len() >= self.config.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}
