use std::{
    cmp::min,
    collections::{HashMap, VecDeque},
    panic::{catch_unwind, AssertUnwindSafe},
    time::{Duration, Instant},
};

use ed25519_dalek::{Signature, SigningKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH};
use rand_core::OsRng;
use sha2::Digest;
use x25519_dalek::StaticSecret;

use crate::{
    buffer::OutputBuffer,
    channel::{
        ChannelError, Envelope as ChannelEnvelope, Handler as ChannelHandler, HandlerId,
        MessageState as ChannelMessageState,
    },
    crypt::fernet::{CachedFernet, PlainText, Token},
    error::RnsError,
    hash::{AddressHash, Hash, ADDRESS_HASH_SIZE, HASH_SIZE},
    identity::{DecryptIdentity, DerivedKey, EncryptIdentity, Identity, PrivateIdentity},
    packet::{
        DestinationType, Header, Packet, PacketContext, PacketDataBuffer, PacketType, PACKET_MDU,
    },
};

use super::DestinationDesc;

const LINK_MTU_SIZE: usize = 3;
const KEEPALIVE_MAX_RTT: f32 = 1.75;
const KEEPALIVE_TIMEOUT_FACTOR: f32 = 4.0;
const STALE_GRACE_SECS: f32 = 5.0;
const KEEPALIVE_MAX_SECS: f32 = 360.0;
const KEEPALIVE_MIN_SECS: f32 = 5.0;
const STALE_FACTOR: f32 = 2.0;
const CHANNEL_RX_WINDOW_MAX: u16 = 48;
const CHANNEL_WINDOW_INIT: u8 = 2;
const CHANNEL_WINDOW_MIN: u8 = 2;
const CHANNEL_WINDOW_MIN_LIMIT_MEDIUM: u8 = 5;
const CHANNEL_WINDOW_MIN_LIMIT_FAST: u8 = 16;
const CHANNEL_WINDOW_MAX_SLOW: u8 = 5;
const CHANNEL_WINDOW_MAX_MEDIUM: u8 = 12;
const CHANNEL_WINDOW_MAX_FAST: u8 = 48;
const CHANNEL_FAST_RATE_THRESHOLD: u8 = 10;
const CHANNEL_RTT_FAST_SECS: f32 = 0.18;
const CHANNEL_RTT_MEDIUM_SECS: f32 = 0.75;
const CHANNEL_RTT_SLOW_SECS: f32 = 1.45;
const CHANNEL_WINDOW_FLEXIBILITY: u8 = 4;
const CHANNEL_MAX_TRIES: u8 = 5;

#[derive(Debug, Copy, Clone)]
struct PendingChannelPacket {
    sequence: u16,
    packet: Packet,
    tries: u8,
    next_retry_at: Instant,
}

struct RegisteredChannelHandler {
    id: HandlerId,
    handler: ChannelHandler,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum LinkStatus {
    Pending = 0x00,
    Handshake = 0x01,
    Active = 0x02,
    Stale = 0x03,
    Closed = 0x04,
}

impl LinkStatus {
    pub fn not_yet_active(&self) -> bool {
        *self == LinkStatus::Pending || *self == LinkStatus::Handshake
    }
}

pub type LinkId = AddressHash;

include!("link/payload.rs");
include!("link/id.rs");

#[allow(clippy::large_enum_variant)]
pub enum LinkHandleResult {
    None,
    Activated,
    Proof(Packet),
    KeepAlive,
}

#[derive(Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum LinkWatchdogAction {
    None,
    SendKeepAlive,
    SendTeardown(Packet),
}

#[derive(Clone)]
pub enum LinkEvent {
    Activated,
    Data(Box<LinkPayload>),
    Closed,
}

#[derive(Clone)]
pub struct LinkEventData {
    pub id: LinkId,
    pub address_hash: AddressHash,
    pub event: LinkEvent,
}

pub struct Link {
    id: LinkId,
    destination: DestinationDesc,
    ingress_iface: Option<AddressHash>,
    priv_identity: PrivateIdentity,
    peer_identity: Identity,
    derived_key: DerivedKey,
    session_cipher: Option<CachedFernet>,
    signalling: Option<[u8; LINK_MTU_SIZE]>,
    status: LinkStatus,
    request_time: Instant,
    rtt: Duration,
    activated_at: Option<Instant>,
    last_inbound: Option<Instant>,
    last_outbound: Option<Instant>,
    last_data: Option<Instant>,
    last_keepalive: Option<Instant>,
    last_proof: Option<Instant>,
    stale_since: Option<Instant>,
    keepalive: Duration,
    stale_time: Duration,
    next_channel_sequence: u16,
    next_channel_rx_sequence: u16,
    channel_open: bool,
    next_channel_handler_id: u64,
    channel_handlers: HashMap<u16, Vec<RegisteredChannelHandler>>,
    channel_pending: HashMap<Hash, PendingChannelPacket>,
    channel_states: HashMap<u16, ChannelMessageState>,
    channel_rx_ring: HashMap<u16, ChannelEnvelope>,
    channel_window: u8,
    channel_window_max: u8,
    channel_window_min: u8,
    channel_window_flexibility: u8,
    channel_fast_rate_rounds: u8,
    channel_medium_rate_rounds: u8,
    event_tx: tokio::sync::broadcast::Sender<LinkEventData>,
}

impl Link {
    pub fn new(
        destination: DestinationDesc,
        event_tx: tokio::sync::broadcast::Sender<LinkEventData>,
    ) -> Self {
        Self {
            id: AddressHash::new_empty(),
            destination,
            ingress_iface: None,
            priv_identity: PrivateIdentity::new_from_rand(OsRng),
            peer_identity: Identity::default(),
            derived_key: DerivedKey::new_empty(),
            session_cipher: None,
            signalling: None,
            status: LinkStatus::Pending,
            request_time: Instant::now(),
            rtt: Duration::from_secs(0),
            activated_at: None,
            last_inbound: None,
            last_outbound: None,
            last_data: None,
            last_keepalive: None,
            last_proof: None,
            stale_since: None,
            keepalive: Duration::from_secs_f32(KEEPALIVE_MAX_SECS),
            stale_time: Duration::from_secs_f32(KEEPALIVE_MAX_SECS * STALE_FACTOR),
            next_channel_sequence: 0,
            next_channel_rx_sequence: 0,
            channel_open: false,
            next_channel_handler_id: 0,
            channel_handlers: HashMap::new(),
            channel_pending: HashMap::new(),
            channel_states: HashMap::new(),
            channel_rx_ring: HashMap::new(),
            channel_window: CHANNEL_WINDOW_INIT,
            channel_window_max: CHANNEL_WINDOW_MAX_SLOW,
            channel_window_min: CHANNEL_WINDOW_MIN,
            channel_window_flexibility: CHANNEL_WINDOW_FLEXIBILITY,
            channel_fast_rate_rounds: 0,
            channel_medium_rate_rounds: 0,
            event_tx,
        }
    }

    pub fn new_from_request(
        packet: &Packet,
        signing_key: SigningKey,
        destination: DestinationDesc,
        event_tx: tokio::sync::broadcast::Sender<LinkEventData>,
    ) -> Result<Self, RnsError> {
        if packet.data.len() < PUBLIC_KEY_LENGTH * 2 {
            return Err(RnsError::InvalidArgument);
        }

        let data = packet.data.as_slice();
        let peer_identity = Identity::new_from_slices(
            &data[..PUBLIC_KEY_LENGTH],
            &data[PUBLIC_KEY_LENGTH..PUBLIC_KEY_LENGTH * 2],
        );
        let signalling = if data.len() >= PUBLIC_KEY_LENGTH * 2 + LINK_MTU_SIZE {
            let mut bytes = [0u8; LINK_MTU_SIZE];
            bytes.copy_from_slice(
                &data[PUBLIC_KEY_LENGTH * 2..PUBLIC_KEY_LENGTH * 2 + LINK_MTU_SIZE],
            );
            Some(bytes)
        } else {
            None
        };

        let link_id = LinkId::from(packet);
        log::debug!("link: create from request {}", link_id);

        let mut link = Self {
            id: link_id,
            destination,
            ingress_iface: None,
            priv_identity: PrivateIdentity::new(StaticSecret::random_from_rng(OsRng), signing_key),
            peer_identity,
            derived_key: DerivedKey::new_empty(),
            session_cipher: None,
            signalling,
            status: LinkStatus::Pending,
            request_time: Instant::now(),
            rtt: Duration::from_secs(0),
            activated_at: None,
            last_inbound: None,
            last_outbound: None,
            last_data: None,
            last_keepalive: None,
            last_proof: None,
            stale_since: None,
            keepalive: Duration::from_secs_f32(KEEPALIVE_MAX_SECS),
            stale_time: Duration::from_secs_f32(KEEPALIVE_MAX_SECS * STALE_FACTOR),
            next_channel_sequence: 0,
            next_channel_rx_sequence: 0,
            channel_open: false,
            next_channel_handler_id: 0,
            channel_handlers: HashMap::new(),
            channel_pending: HashMap::new(),
            channel_states: HashMap::new(),
            channel_rx_ring: HashMap::new(),
            channel_window: CHANNEL_WINDOW_INIT,
            channel_window_max: CHANNEL_WINDOW_MAX_SLOW,
            channel_window_min: CHANNEL_WINDOW_MIN,
            channel_window_flexibility: CHANNEL_WINDOW_FLEXIBILITY,
            channel_fast_rate_rounds: 0,
            channel_medium_rate_rounds: 0,
            event_tx,
        };

        link.handshake(peer_identity);

        Ok(link)
    }

    pub fn request(&mut self) -> Packet {
        if self.status != LinkStatus::Pending {
            self.refresh_local_identity();
        }

        let mut packet_data = PacketDataBuffer::new();

        packet_data.safe_write(self.priv_identity.as_identity().public_key.as_bytes());
        packet_data.safe_write(self.priv_identity.as_identity().verifying_key.as_bytes());

        let packet = Packet {
            header: Header { packet_type: PacketType::LinkRequest, ..Default::default() },
            ifac: None,
            destination: self.destination.address_hash,
            transport: None,
            context: PacketContext::None,
            data: packet_data,
        };

        self.status = LinkStatus::Pending;
        self.id = LinkId::from(&packet);
        self.derived_key = DerivedKey::new_empty();
        self.session_cipher = None;
        self.request_time = Instant::now();
        self.activated_at = None;
        self.ingress_iface = None;
        self.last_inbound = None;
        self.last_outbound = Some(self.request_time);
        self.last_data = Some(self.request_time);
        self.last_keepalive = None;
        self.last_proof = None;
        self.stale_since = None;
        self.keepalive = Duration::from_secs_f32(KEEPALIVE_MAX_SECS);
        self.stale_time = Duration::from_secs_f32(KEEPALIVE_MAX_SECS * STALE_FACTOR);
        self.next_channel_sequence = 0;
        self.next_channel_rx_sequence = 0;
        self.channel_open = false;
        self.channel_pending.clear();
        self.channel_states.clear();
        self.channel_rx_ring.clear();
        self.reset_channel_flow_control();

        packet
    }

    pub fn prove(&mut self) -> Packet {
        log::debug!("link({}): prove", self.id);

        if self.status != LinkStatus::Active {
            self.status = LinkStatus::Active;
            let activated_at = Instant::now();
            self.activated_at = Some(activated_at);
            self.last_proof = Some(activated_at);
            self.stale_since = None;
            self.post_event(LinkEvent::Activated);
        }

        let mut packet_data = PacketDataBuffer::new();

        packet_data.safe_write(self.id.as_slice());
        packet_data.safe_write(self.priv_identity.as_identity().public_key.as_bytes());
        packet_data.safe_write(self.priv_identity.as_identity().verifying_key.as_bytes());
        if let Some(signalling) = self.signalling {
            packet_data.safe_write(&signalling);
        }

        let signature = self.priv_identity.sign(packet_data.as_slice());

        packet_data.reset();
        packet_data.safe_write(&signature.to_bytes()[..]);
        packet_data.safe_write(self.priv_identity.as_identity().public_key.as_bytes());
        if let Some(signalling) = self.signalling {
            packet_data.safe_write(&signalling);
        }

        Packet {
            header: Header {
                packet_type: PacketType::Proof,
                destination_type: DestinationType::Link,
                hops: 0,
                ..Default::default()
            },
            ifac: None,
            destination: self.id,
            transport: None,
            context: PacketContext::LinkRequestProof,
            data: packet_data,
        }
    }

    pub fn prove_packet(&self, packet: &Packet) -> Packet {
        let hash = packet.hash().to_bytes();
        let signature = self.priv_identity.sign(&hash).to_bytes();
        let mut packet_data = PacketDataBuffer::new();

        packet_data.safe_write(&hash);
        packet_data.safe_write(&signature);

        Packet {
            header: Header {
                packet_type: PacketType::Proof,
                destination_type: DestinationType::Link,
                ..Default::default()
            },
            ifac: None,
            destination: self.id,
            transport: None,
            context: PacketContext::LinkProof,
            data: packet_data,
        }
    }

    fn handle_data_packet(&mut self, packet: &Packet) -> LinkHandleResult {
        if self.status != LinkStatus::Active {
            log::warn!("link({}): handling data packet in inactive state", self.id);
        }
        self.note_inbound(packet.context);

        match packet.context {
            PacketContext::Channel => {
                if !self.channel_is_open() {
                    log::debug!("link({}): channel data received without open channel", self.id);
                    return LinkHandleResult::None;
                }

                let proof = self.prove_packet(packet);
                let mut buffer = [0u8; PACKET_MDU];
                if let Ok(plain_text) = self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    log::trace!("link({}): data {}B", self.id, plain_text.len());
                    self.handle_channel_frame(plain_text);
                } else {
                    log::error!("link({}): can't decrypt packet", self.id);
                }
                return LinkHandleResult::Proof(proof);
            }
            PacketContext::None
            | PacketContext::Request
            | PacketContext::Response
            | PacketContext::LinkIdentify => {
                let mut buffer = [0u8; PACKET_MDU];
                if let Ok(plain_text) = self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    log::trace!("link({}): data {}B", self.id, plain_text.len());
                    let request_id = if packet.context == PacketContext::Request {
                        let hash = packet.hash().to_bytes();
                        let mut id = [0u8; ADDRESS_HASH_SIZE];
                        id.copy_from_slice(&hash[..ADDRESS_HASH_SIZE]);
                        Some(id)
                    } else {
                        None
                    };
                    self.post_event(LinkEvent::Data(Box::new(
                        LinkPayload::new_from_slice_with_context_and_request_id(
                            plain_text,
                            packet.context,
                            request_id,
                        ),
                    )));
                    if packet.context == PacketContext::None {
                        return LinkHandleResult::Proof(self.prove_packet(packet));
                    }
                    return LinkHandleResult::None;
                } else {
                    log::error!("link({}): can't decrypt packet", self.id);
                }
            }
            PacketContext::KeepAlive => {
                if !packet.data.is_empty() && packet.data.as_slice()[0] == 0xFF {
                    self.request_time = Instant::now();
                    log::trace!("link({}): keep-alive request", self.id);
                    return LinkHandleResult::KeepAlive;
                }
                if !packet.data.is_empty() && packet.data.as_slice()[0] == 0xFE {
                    log::trace!("link({}): keep-alive response", self.id);
                    return LinkHandleResult::None;
                }
            }
            PacketContext::LinkClose => {
                let mut buffer = [0u8; PACKET_MDU];
                if let Ok(plain_text) = self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    if plain_text == self.id.as_slice() {
                        self.finalize_local_close();
                    }
                }
                return LinkHandleResult::None;
            }
            PacketContext::LinkRTT => {
                let mut buffer = [0u8; PACKET_MDU];
                if let Ok(plain_text) = self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    let mut cursor = std::io::Cursor::new(plain_text);
                    if let Ok(peer_rtt) = rmp::decode::read_f32(&mut cursor) {
                        let measured_rtt = self.request_time.elapsed().as_secs_f32();
                        self.rtt = Duration::from_secs_f32(measured_rtt.max(peer_rtt));
                        self.update_keepalive_timing();
                        self.refresh_channel_flow_control();
                        if self.activated_at.is_none() {
                            self.activated_at = Some(Instant::now());
                        }
                    }
                }
            }
            _ => {}
        }

        LinkHandleResult::None
    }

    fn iface_matches(&self, iface: AddressHash) -> bool {
        if let Some(expected_iface) = self.ingress_iface {
            if expected_iface != iface {
                log::warn!(
                    "link({}): dropping packet from iface {} expected {}",
                    self.id,
                    iface,
                    expected_iface
                );
                return false;
            }
        }

        true
    }

    pub fn handle_packet(&mut self, packet: &Packet, iface: AddressHash) -> LinkHandleResult {
        if packet.destination != self.id {
            return LinkHandleResult::None;
        }
        if !self.iface_matches(iface) {
            return LinkHandleResult::None;
        }

        match packet.header.packet_type {
            PacketType::Data => return self.handle_data_packet(packet),
            PacketType::Proof => {
                if self.status == LinkStatus::Active && packet.context == PacketContext::LinkProof {
                    if let Ok(hash) = self.validate_packet_proof(packet) {
                        self.note_inbound(packet.context);
                        if let Some(pending) = self.channel_pending.remove(&hash) {
                            self.channel_states
                                .insert(pending.sequence, ChannelMessageState::Delivered);
                            self.note_channel_delivery();
                        }
                        return LinkHandleResult::None;
                    }
                }
                if self.status == LinkStatus::Pending
                    && packet.context == PacketContext::LinkRequestProof
                {
                    if let Ok(identity) =
                        validate_link_request_proof_packet(&self.destination, &self.id, packet)
                    {
                        log::debug!("link({}): has been proved", self.id);

                        self.handshake(identity);
                        self.ingress_iface.get_or_insert(iface);

                        self.status = LinkStatus::Active;
                        self.rtt = self.request_time.elapsed();
                        self.activated_at = Some(Instant::now());
                        self.last_proof = self.activated_at;
                        self.stale_since = None;
                        self.update_keepalive_timing();
                        self.refresh_channel_flow_control();

                        log::debug!("link({}): activated", self.id);

                        self.post_event(LinkEvent::Activated);

                        return LinkHandleResult::Activated;
                    } else {
                        log::warn!("link({}): proof is not valid", self.id);
                    }
                }
            }
            _ => {}
        }

        LinkHandleResult::None
    }

    pub fn data_packet(&self, data: &[u8]) -> Result<Packet, RnsError> {
        self.packet_with_context(data, PacketContext::None)
    }

    pub fn channel_packet(&self, data: &[u8]) -> Result<Packet, RnsError> {
        self.packet_with_context(data, PacketContext::Channel)
    }

    pub fn register_channel_handler<F>(&mut self, msg_type: u16, handler: F) -> HandlerId
    where
        F: FnMut(ChannelEnvelope) -> bool + Send + 'static,
    {
        self.channel_open = true;
        let id = HandlerId::new(self.next_channel_handler_id);
        self.next_channel_handler_id = self.next_channel_handler_id.wrapping_add(1);
        self.channel_handlers
            .entry(msg_type)
            .or_default()
            .push(RegisteredChannelHandler { id, handler: Box::new(handler) });
        id
    }

    pub fn remove_channel_handler(&mut self, handler_id: HandlerId) -> bool {
        let mut empty_msg_types = Vec::new();
        let mut removed = false;

        for (msg_type, handlers) in &mut self.channel_handlers {
            let before = handlers.len();
            handlers.retain(|registered| registered.id != handler_id);
            if handlers.is_empty() {
                empty_msg_types.push(*msg_type);
            }
            if handlers.len() != before {
                removed = true;
            }
        }

        for msg_type in empty_msg_types {
            self.channel_handlers.remove(&msg_type);
        }

        removed
    }

    pub fn send_channel_message(
        &mut self,
        msg_type: u16,
        payload: Vec<u8>,
    ) -> Result<(u16, Packet), ChannelError> {
        if self.status != LinkStatus::Active {
            return Err(ChannelError::LinkNotReady);
        }
        self.channel_open = true;
        if self.channel_pending.len() >= self.channel_send_window() {
            return Err(ChannelError::LinkNotReady);
        }

        let sequence = self.next_channel_sequence;
        self.next_channel_sequence = self.next_channel_sequence.wrapping_add(1);
        let envelope = ChannelEnvelope { msg_type, sequence, payload };
        let raw = envelope.pack();
        let packet = self.channel_packet(&raw).map_err(|_| ChannelError::PayloadTooLarge)?;
        self.channel_pending.insert(
            packet.hash(),
            PendingChannelPacket {
                sequence,
                packet,
                tries: 1,
                next_retry_at: Instant::now()
                    + Self::channel_retry_timeout_for(self.rtt, 1, self.channel_pending.len() + 1),
            },
        );
        self.channel_states.insert(sequence, ChannelMessageState::Sent);
        Ok((sequence, packet))
    }

    pub fn channel_state(&self, sequence: u16) -> ChannelMessageState {
        self.channel_states.get(&sequence).copied().unwrap_or(ChannelMessageState::New)
    }

    pub fn open_channel(&mut self) {
        self.channel_open = true;
    }

    pub fn close_channel(&mut self) {
        self.channel_open = false;
    }

    pub(crate) fn mark_channel_failed(&mut self, sequence: u16) {
        if let Some(hash) = self
            .channel_pending
            .iter()
            .find_map(|(hash, pending)| (pending.sequence == sequence).then_some(*hash))
        {
            self.channel_pending.remove(&hash);
        }
        self.channel_states.insert(sequence, ChannelMessageState::Failed);
    }

    pub(crate) fn poll_channel_timeouts(&mut self, now: Instant) -> Vec<Packet> {
        if !matches!(self.status, LinkStatus::Active | LinkStatus::Stale) {
            return Vec::new();
        }

        let timed_out = self
            .channel_pending
            .iter()
            .filter_map(|(hash, pending)| (pending.next_retry_at <= now).then_some(*hash))
            .collect::<Vec<_>>();
        if timed_out.is_empty() {
            return Vec::new();
        }

        let outstanding = self.channel_pending.len().max(1);
        let rtt = self.rtt;
        let mut resend_packets = Vec::new();
        let mut exhausted = false;

        for hash in timed_out {
            self.note_channel_timeout();
            if let Some(pending) = self.channel_pending.get_mut(&hash) {
                if pending.tries >= CHANNEL_MAX_TRIES {
                    exhausted = true;
                    break;
                }

                pending.tries += 1;
                let tries = pending.tries;
                let retry_timeout = Self::channel_retry_timeout_for(rtt, tries, outstanding);
                pending.next_retry_at = now + retry_timeout;
                resend_packets.push(pending.packet);
            }
        }

        if exhausted {
            for pending in self.channel_pending.drain().map(|(_, pending)| pending) {
                self.channel_states.insert(pending.sequence, ChannelMessageState::Failed);
            }
            self.close();
            return Vec::new();
        }

        resend_packets
    }

    pub(crate) fn next_channel_retry_at(&self) -> Option<Instant> {
        if !matches!(self.status, LinkStatus::Active | LinkStatus::Stale) {
            return None;
        }

        self.channel_pending.values().map(|pending| pending.next_retry_at).min()
    }

    fn channel_send_window(&self) -> usize {
        usize::from(self.channel_window)
    }

    pub fn channel_ready_to_send(&self) -> bool {
        self.status == LinkStatus::Active
            && self.ingress_iface.is_some()
            && self.channel_pending.len() < self.channel_send_window()
    }

    pub fn channel_close_wait_hint(&self) -> Duration {
        Duration::from_secs_f32(self.rtt.as_secs_f32() * self.channel_pending.len() as f32)
    }
    fn channel_retry_timeout_for(rtt: Duration, tries: u8, outstanding: usize) -> Duration {
        let base = (rtt.as_secs_f32() * 2.5).max(0.025);
        let multiplier = 1.5_f32.powi(i32::from(tries.saturating_sub(1)));
        Duration::from_secs_f32(multiplier * base * (outstanding as f32 + 1.5))
    }

    fn channel_window_profile(rtt: Duration) -> (u8, u8, u8, u8) {
        if rtt.as_secs_f32() > CHANNEL_RTT_SLOW_SECS {
            (1, 1, 1, 1)
        } else {
            (
                CHANNEL_WINDOW_INIT,
                CHANNEL_WINDOW_MAX_SLOW,
                CHANNEL_WINDOW_MIN,
                CHANNEL_WINDOW_FLEXIBILITY,
            )
        }
    }

    fn reset_channel_flow_control(&mut self) {
        let (window, window_max, window_min, flexibility) = Self::channel_window_profile(self.rtt);
        self.channel_window = window;
        self.channel_window_max = window_max;
        self.channel_window_min = window_min;
        self.channel_window_flexibility = flexibility;
        self.channel_fast_rate_rounds = 0;
        self.channel_medium_rate_rounds = 0;
    }

    fn refresh_channel_flow_control(&mut self) {
        let (window, window_max, window_min, flexibility) = Self::channel_window_profile(self.rtt);
        self.channel_window_max = window_max;
        self.channel_window_min = window_min;
        self.channel_window_flexibility = flexibility;
        if self.channel_window < self.channel_window_min || self.channel_window == 0 {
            self.channel_window = self.channel_window_min.max(window);
        }
        if self.channel_window > self.channel_window_max {
            self.channel_window = self.channel_window_max;
        }
    }

    fn note_channel_delivery(&mut self) {
        if self.channel_window < self.channel_window_max {
            self.channel_window += 1;
        }

        if self.rtt.is_zero() {
            return;
        }

        if self.rtt.as_secs_f32() > CHANNEL_RTT_FAST_SECS {
            self.channel_fast_rate_rounds = 0;

            if self.rtt.as_secs_f32() > CHANNEL_RTT_MEDIUM_SECS {
                self.channel_medium_rate_rounds = 0;
            } else {
                self.channel_medium_rate_rounds = self.channel_medium_rate_rounds.saturating_add(1);
                if self.channel_window_max < CHANNEL_WINDOW_MAX_MEDIUM
                    && self.channel_medium_rate_rounds == CHANNEL_FAST_RATE_THRESHOLD
                {
                    self.channel_window_max = CHANNEL_WINDOW_MAX_MEDIUM;
                    self.channel_window_min = CHANNEL_WINDOW_MIN_LIMIT_MEDIUM;
                    if self.channel_window < self.channel_window_min {
                        self.channel_window = self.channel_window_min;
                    }
                }
            }
        } else {
            self.channel_fast_rate_rounds = self.channel_fast_rate_rounds.saturating_add(1);
            if self.channel_window_max < CHANNEL_WINDOW_MAX_FAST
                && self.channel_fast_rate_rounds == CHANNEL_FAST_RATE_THRESHOLD
            {
                self.channel_window_max = CHANNEL_WINDOW_MAX_FAST;
                self.channel_window_min = CHANNEL_WINDOW_MIN_LIMIT_FAST;
                if self.channel_window < self.channel_window_min {
                    self.channel_window = self.channel_window_min;
                }
            }
        }
    }

    fn note_channel_timeout(&mut self) {
        self.channel_fast_rate_rounds = 0;
        self.channel_medium_rate_rounds = 0;

        if self.channel_window > self.channel_window_min {
            self.channel_window -= 1;
        }
        if self.channel_window_max > self.channel_window_min + self.channel_window_flexibility {
            self.channel_window_max -= 1;
        }
        if self.channel_window > self.channel_window_max {
            self.channel_window = self.channel_window_max;
        }
    }

    fn packet_with_context(&self, data: &[u8], context: PacketContext) -> Result<Packet, RnsError> {
        if self.status != LinkStatus::Active {
            log::warn!("link: can't create data packet for closed link");
        }

        let mut packet_data = PacketDataBuffer::new();
        self.encrypt_packet_data_into(data, &mut packet_data)?;

        Ok(Packet {
            header: Header {
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            ifac: None,
            destination: self.id,
            transport: None,
            context,
            data: packet_data,
        })
    }

    pub fn data_packet_into(&self, data: &[u8], packet: &mut Packet) -> Result<(), RnsError> {
        if self.status != LinkStatus::Active {
            log::warn!("link: can't create data packet for closed link");
        }

        packet.header = Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Data,
            ..Default::default()
        };
        packet.ifac = None;
        packet.destination = self.id;
        packet.transport = None;
        packet.context = PacketContext::None;
        self.encrypt_packet_data_into(data, &mut packet.data)
    }

    pub fn keep_alive_packet(&self, data: u8) -> Packet {
        log::trace!("link({}): create keep alive {}", self.id, data);

        let mut packet_data = PacketDataBuffer::new();
        packet_data.safe_write(&[data]);

        Packet {
            header: Header {
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            ifac: None,
            destination: self.id,
            transport: None,
            context: PacketContext::KeepAlive,
            data: packet_data,
        }
    }

    pub fn encrypt<'a>(&self, text: &[u8], out_buf: &'a mut [u8]) -> Result<&'a [u8], RnsError> {
        if let Some(session_cipher) = &self.session_cipher {
            let token = session_cipher.encrypt(OsRng, PlainText::from(text), out_buf)?;
            Ok(token.as_bytes())
        } else {
            self.priv_identity.encrypt(OsRng, text, &self.derived_key, out_buf)
        }
    }

    pub fn decrypt<'a>(&self, text: &[u8], out_buf: &'a mut [u8]) -> Result<&'a [u8], RnsError> {
        if let Some(session_cipher) = &self.session_cipher {
            let verified = session_cipher.verify(Token::from(text))?;
            let plain_text = session_cipher.decrypt(verified, out_buf)?;
            Ok(plain_text.as_bytes())
        } else {
            self.priv_identity.decrypt(OsRng, text, &self.derived_key, out_buf)
        }
    }

    pub fn destination(&self) -> &DestinationDesc {
        &self.destination
    }

    pub fn ingress_iface(&self) -> Option<AddressHash> {
        self.ingress_iface
    }

    pub fn set_ingress_iface(&mut self, iface: AddressHash) {
        self.ingress_iface = Some(iface);
    }

    pub fn peer_identity(&self) -> &Identity {
        &self.peer_identity
    }

    pub fn create_rtt(&self) -> Packet {
        let rtt = self.rtt.as_secs_f32();
        let mut buf = Vec::new();
        {
            buf.reserve(4);
            rmp::encode::write_f32(&mut buf, rtt).unwrap();
        }

        let mut packet_data = PacketDataBuffer::new();

        let token_len = {
            let token = self
                .encrypt(buf.as_slice(), packet_data.accuire_buf_max())
                .expect("encrypted data");
            token.len()
        };

        packet_data.resize(token_len);

        log::trace!("link: {} create rtt packet = {} sec", self.id, rtt);

        Packet {
            header: Header { destination_type: DestinationType::Link, ..Default::default() },
            ifac: None,
            destination: self.id,
            transport: None,
            context: PacketContext::LinkRTT,
            data: packet_data,
        }
    }

    fn refresh_local_identity(&mut self) {
        self.priv_identity = PrivateIdentity::new(
            StaticSecret::random_from_rng(OsRng),
            self.priv_identity.sign_key().clone(),
        );
    }

    fn handshake(&mut self, peer_identity: Identity) {
        log::debug!("link({}): handshake", self.id);

        self.status = LinkStatus::Handshake;
        self.peer_identity = peer_identity;

        self.derived_key =
            self.priv_identity.derive_key(&self.peer_identity.public_key, Some(self.id.as_slice()));
        let key_bytes = self.derived_key.as_bytes();
        let split = key_bytes.len() / 2;
        self.session_cipher =
            Some(CachedFernet::new_from_slices(&key_bytes[..split], &key_bytes[split..]));
    }

    fn note_inbound(&mut self, context: PacketContext) {
        let now = Instant::now();
        self.last_inbound = Some(now);
        if self.status == LinkStatus::Stale {
            self.status = LinkStatus::Active;
            self.stale_since = None;
        }
        if context != PacketContext::KeepAlive {
            self.last_data = Some(now);
            self.request_time = now;
        }
    }

    pub(crate) fn note_outbound(&mut self, context: PacketContext) {
        let now = Instant::now();
        self.last_outbound = Some(now);
        if context == PacketContext::KeepAlive {
            self.last_keepalive = Some(now);
        } else {
            self.last_data = Some(now);
        }
    }

    fn update_keepalive_timing(&mut self) {
        let keepalive_secs = (self.rtt.as_secs_f32() * (KEEPALIVE_MAX_SECS / KEEPALIVE_MAX_RTT))
            .clamp(KEEPALIVE_MIN_SECS, KEEPALIVE_MAX_SECS);
        self.keepalive = Duration::from_secs_f32(keepalive_secs);
        self.stale_time = Duration::from_secs_f32(keepalive_secs * STALE_FACTOR);
    }

    fn inbound_anchor(&self) -> Instant {
        [self.activated_at, self.last_proof, self.last_inbound]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(self.request_time)
    }

    fn activity_anchor(&self, last_activity: Option<Instant>) -> Instant {
        [self.activated_at, last_activity].into_iter().flatten().max().unwrap_or(self.request_time)
    }

    pub fn no_inbound_for(&self) -> Duration {
        Instant::now().duration_since(self.activity_anchor(self.last_inbound))
    }

    pub fn no_outbound_for(&self) -> Duration {
        Instant::now().duration_since(self.activity_anchor(self.last_outbound))
    }

    pub fn no_data_for(&self) -> Duration {
        Instant::now().duration_since(self.activity_anchor(self.last_data))
    }

    pub fn inactive_for(&self) -> Duration {
        self.no_inbound_for().min(self.no_outbound_for())
    }

    pub fn next_watchdog_deadline(&self, initiator: bool) -> Option<Instant> {
        match self.status {
            LinkStatus::Active => {
                let inbound_anchor = self.inbound_anchor();
                let keepalive_due_at = inbound_anchor + self.keepalive;
                if keepalive_due_at > Instant::now() {
                    return Some(keepalive_due_at);
                }

                let stale_due_at = inbound_anchor + self.stale_time;
                if !initiator {
                    return Some(stale_due_at);
                }

                let keepalive_anchor = self.last_keepalive.unwrap_or(inbound_anchor);
                Some((keepalive_anchor + self.keepalive).min(stale_due_at))
            }
            LinkStatus::Stale => self.stale_since.map(|stale_since| {
                stale_since
                    + Duration::from_secs_f32(
                        (self.rtt.as_secs_f32() * KEEPALIVE_TIMEOUT_FACTOR) + STALE_GRACE_SECS,
                    )
            }),
            _ => None,
        }
    }

    pub fn check_watchdog(&mut self, initiator: bool) -> LinkWatchdogAction {
        let now = Instant::now();
        match self.status {
            LinkStatus::Active => {
                let inbound_anchor = self.inbound_anchor();
                let keepalive_due = now.duration_since(inbound_anchor) >= self.keepalive;
                if keepalive_due {
                    if now.duration_since(inbound_anchor) >= self.stale_time {
                        self.status = LinkStatus::Stale;
                        self.stale_since = Some(now);
                    }

                    if initiator {
                        let keepalive_anchor = self.last_keepalive.unwrap_or(inbound_anchor);
                        if now.duration_since(keepalive_anchor) >= self.keepalive {
                            return LinkWatchdogAction::SendKeepAlive;
                        }
                    }
                }
                LinkWatchdogAction::None
            }
            LinkStatus::Stale => {
                let stale_timeout = Duration::from_secs_f32(
                    (self.rtt.as_secs_f32() * KEEPALIVE_TIMEOUT_FACTOR) + STALE_GRACE_SECS,
                );
                if let Some(stale_since) = self.stale_since {
                    if now.duration_since(stale_since) >= stale_timeout {
                        if let Some(packet) = self.teardown() {
                            return LinkWatchdogAction::SendTeardown(packet);
                        }
                    }
                }
                LinkWatchdogAction::None
            }
            _ => LinkWatchdogAction::None,
        }
    }

    fn encrypt_packet_data_into(
        &self,
        data: &[u8],
        packet_data: &mut PacketDataBuffer,
    ) -> Result<(), RnsError> {
        packet_data.reset();
        let cipher_text_len = {
            let cipher_text = self.encrypt(data, packet_data.accuire_buf_max())?;
            cipher_text.len()
        };
        packet_data.resize(cipher_text_len);
        Ok(())
    }

    fn post_event(&self, event: LinkEvent) {
        let _ = self.event_tx.send(LinkEventData {
            id: self.id,
            address_hash: self.destination.address_hash,
            event,
        });
    }

    fn finalize_local_close(&mut self) {
        for pending in self.channel_pending.drain().map(|(_, pending)| pending) {
            self.channel_states.insert(pending.sequence, ChannelMessageState::Failed);
        }
        self.channel_rx_ring.clear();
        self.channel_open = false;
        self.status = LinkStatus::Closed;
        self.peer_identity = Identity::default();
        self.derived_key = DerivedKey::new_empty();
        self.session_cipher = None;
        self.last_keepalive = None;
        self.last_proof = None;
        self.stale_since = None;
        self.next_channel_sequence = 0;
        self.next_channel_rx_sequence = 0;
        self.reset_channel_flow_control();

        self.post_event(LinkEvent::Closed);

        log::warn!("link: close {}", self.id);
    }

    fn teardown_packet(&self) -> Result<Packet, RnsError> {
        self.packet_with_context(self.id.as_slice(), PacketContext::LinkClose)
    }

    pub fn teardown(&mut self) -> Option<Packet> {
        let packet = if matches!(self.status, LinkStatus::Active | LinkStatus::Stale) {
            self.teardown_packet().ok()
        } else {
            None
        };
        if packet.is_some() {
            self.note_outbound(PacketContext::LinkClose);
        }
        self.finalize_local_close();
        packet
    }

    pub fn close(&mut self) {
        self.finalize_local_close();
    }

    pub fn restart(&mut self) {
        log::warn!("link({}): restart after {}s", self.id, self.request_time.elapsed().as_secs());

        for pending in self.channel_pending.drain().map(|(_, pending)| pending) {
            self.channel_states.insert(pending.sequence, ChannelMessageState::Failed);
        }
        self.channel_rx_ring.clear();
        self.status = LinkStatus::Pending;
        self.peer_identity = Identity::default();
        self.derived_key = DerivedKey::new_empty();
        self.session_cipher = None;
        self.activated_at = None;
        self.ingress_iface = None;
        self.last_inbound = None;
        self.last_outbound = None;
        self.last_data = None;
        self.last_keepalive = None;
        self.last_proof = None;
        self.stale_since = None;
        self.next_channel_rx_sequence = 0;
        self.keepalive = Duration::from_secs_f32(KEEPALIVE_MAX_SECS);
        self.stale_time = Duration::from_secs_f32(KEEPALIVE_MAX_SECS * STALE_FACTOR);
        self.refresh_local_identity();
        self.reset_channel_flow_control();
    }

    pub fn elapsed(&self) -> Duration {
        self.request_time.elapsed()
    }

    pub fn status(&self) -> LinkStatus {
        self.status
    }

    pub fn id(&self) -> &LinkId {
        &self.id
    }

    pub(crate) fn validate_packet_proof(&self, packet: &Packet) -> Result<Hash, RnsError> {
        validate_link_packet_proof(&self.peer_identity, &self.id, packet)
    }

    fn channel_is_open(&self) -> bool {
        self.channel_open || !self.channel_handlers.is_empty()
    }

    fn handle_channel_frame(&mut self, plain_text: &[u8]) -> bool {
        if !self.channel_is_open() {
            return false;
        }

        let Ok(envelope) = ChannelEnvelope::unpack(plain_text) else {
            log::warn!("link({}): invalid channel frame", self.id);
            return false;
        };

        let distance = envelope.sequence.wrapping_sub(self.next_channel_rx_sequence);
        if distance >= 0x8000 {
            log::debug!("link({}): duplicate/old channel frame seq={}", self.id, envelope.sequence);
            return false;
        }
        if distance >= CHANNEL_RX_WINDOW_MAX {
            log::debug!(
                "link({}): channel frame outside receive window seq={} next={}",
                self.id,
                envelope.sequence,
                self.next_channel_rx_sequence
            );
            return false;
        }
        if self.channel_rx_ring.insert(envelope.sequence, envelope).is_some() {
            log::debug!(
                "link({}): duplicate buffered channel frame seq={}",
                self.id,
                self.next_channel_rx_sequence
            );
            return false;
        }

        let mut ready = VecDeque::new();
        while let Some(envelope) = self.channel_rx_ring.remove(&self.next_channel_rx_sequence) {
            ready.push_back(envelope);
            self.next_channel_rx_sequence = self.next_channel_rx_sequence.wrapping_add(1);
        }

        for envelope in ready {
            let Some(handlers) = self.channel_handlers.get_mut(&envelope.msg_type) else {
                log::debug!(
                    "link({}): channel frame without handler type={}",
                    self.id,
                    envelope.msg_type
                );
                continue;
            };
            for registered in handlers {
                match catch_unwind(AssertUnwindSafe(|| (registered.handler)(envelope.clone()))) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(_) => log::error!("link({}): channel handler panicked", self.id),
                }
            }
        }

        true
    }
}

include!("link/proof.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::{DestinationDesc, DestinationName};
    use std::sync::{Arc, Mutex};

    #[test]
    fn link_handshake_roundtrip_encrypts_and_decrypts() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();

        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let proof = inbound.prove();
        let proof_iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(outbound.handle_packet(&proof, proof_iface), LinkHandleResult::Activated));

        let plaintext = b"session-cached-link-payload";
        let mut cipher_buf = [0u8; PACKET_MDU];
        let ciphertext = outbound.encrypt(plaintext, &mut cipher_buf).expect("encrypt");

        let mut plain_buf = [0u8; PACKET_MDU];
        let decrypted = inbound.decrypt(ciphertext, &mut plain_buf).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn pending_request_retries_preserve_link_id() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx);
        let first_request = outbound.request();
        let first_id = *outbound.id();

        let retry_request = outbound.request();

        assert_eq!(first_id, *outbound.id());
        assert_eq!(first_id, LinkId::from(&first_request));
        assert_eq!(first_id, LinkId::from(&retry_request));
    }

    #[test]
    fn outbound_link_binds_to_proof_iface_and_rejects_other_ifaces() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();

        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let proof = inbound.prove();
        let bound_iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(outbound.handle_packet(&proof, bound_iface), LinkHandleResult::Activated));
        assert_eq!(outbound.ingress_iface(), Some(bound_iface));

        let payload = inbound.data_packet(b"hello over the right iface").expect("data packet");

        assert!(matches!(
            outbound.handle_packet(&payload, AddressHash::new_from_rand(OsRng)),
            LinkHandleResult::None
        ));
        assert!(matches!(
            outbound.handle_packet(&payload, bound_iface),
            LinkHandleResult::Proof(_)
        ));
    }

    #[test]
    fn control_context_packets_do_not_auto_generate_link_proofs() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        for context in
            [PacketContext::Request, PacketContext::Response, PacketContext::LinkIdentify]
        {
            let mut packet = inbound.data_packet(b"control-payload").expect("data packet");
            packet.context = context;
            assert!(
                matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::None),
                "{context:?} should not auto-generate a link proof"
            );
        }
    }

    #[test]
    fn channel_packets_do_not_emit_generic_link_events_and_generate_link_proofs() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, mut rx) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));
        while rx.try_recv().is_ok() {}

        outbound.register_channel_handler(0xCAFE, |_| true);

        let (_sequence, packet) = inbound
            .send_channel_message(0xCAFE, b"channel-payload".to_vec())
            .expect("channel packet");

        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
        assert!(rx.try_recv().is_err(), "channel packets should stay on the channel path");
    }

    #[test]
    fn channel_handlers_receive_unpacked_envelopes() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        outbound.register_channel_handler(0x1234, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope);
            true
        });

        let (_sequence, packet) = inbound
            .send_channel_message(0x1234, b"hello-channel".to_vec())
            .expect("channel message");
        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));

        let seen = seen.lock().expect("lock");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].msg_type, 0x1234);
        assert_eq!(seen[0].payload, b"hello-channel");
    }

    #[test]
    fn channel_packets_without_open_handler_are_not_proved() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let (_sequence, packet) =
            inbound.send_channel_message(0xBEEF, b"no-handler".to_vec()).expect("channel message");

        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::None));
    }

    #[test]
    fn explicitly_open_channel_proves_packets_without_handlers() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        outbound.open_channel();

        let (_sequence, packet) = inbound
            .send_channel_message(0xBEEF, b"open-no-handler".to_vec())
            .expect("channel message");

        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
    }

    #[test]
    fn out_of_order_channel_messages_are_buffered_until_contiguous() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        outbound.register_channel_handler(0x4321, move |envelope| {
            seen_clone.lock().expect("lock").push((envelope.sequence, envelope.payload));
            true
        });

        let (_first_sequence, first_packet) =
            inbound.send_channel_message(0x4321, b"first".to_vec()).expect("first channel message");
        let (_second_sequence, second_packet) = inbound
            .send_channel_message(0x4321, b"second".to_vec())
            .expect("second channel message");

        assert!(matches!(
            outbound.handle_packet(&second_packet, iface),
            LinkHandleResult::Proof(_)
        ));
        assert!(seen.lock().expect("lock").is_empty());

        assert!(matches!(outbound.handle_packet(&first_packet, iface), LinkHandleResult::Proof(_)));

        let seen = seen.lock().expect("lock");
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, 0);
        assert_eq!(seen[0].1, b"first");
        assert_eq!(seen[1].0, 1);
        assert_eq!(seen[1].1, b"second");
    }

    #[test]
    fn duplicate_channel_messages_are_ignored() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        outbound.register_channel_handler(0x2468, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope.sequence);
            true
        });

        let (_sequence, packet) =
            inbound.send_channel_message(0x2468, b"dedupe".to_vec()).expect("channel message");

        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));

        let seen = seen.lock().expect("lock");
        assert_eq!(seen.as_slice(), &[0]);
    }

    #[test]
    fn channel_handlers_run_in_registration_order_and_short_circuit() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let calls = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let first_short_circuits = Arc::new(Mutex::new(false));

        let calls_clone = calls.clone();
        let first_flag = first_short_circuits.clone();
        outbound.register_channel_handler(0x5151, move |_| {
            calls_clone.lock().expect("lock").push("first");
            *first_flag.lock().expect("lock")
        });

        let calls_clone = calls.clone();
        outbound.register_channel_handler(0x5151, move |_| {
            calls_clone.lock().expect("lock").push("second");
            true
        });

        let (_sequence, packet) =
            inbound.send_channel_message(0x5151, b"fan-out".to_vec()).expect("channel message");
        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
        assert_eq!(calls.lock().expect("lock").as_slice(), ["first", "second"]);

        calls.lock().expect("lock").clear();
        *first_short_circuits.lock().expect("lock") = true;

        let (_sequence, packet) = inbound
            .send_channel_message(0x5151, b"short-circuit".to_vec())
            .expect("channel message");
        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
        assert_eq!(calls.lock().expect("lock").as_slice(), ["first"]);
    }

    #[test]
    fn removing_last_channel_handler_keeps_explicit_channel_open_state() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        let handler_id = outbound.register_channel_handler(0x6161, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope);
            true
        });
        assert!(outbound.remove_channel_handler(handler_id));
        assert!(!outbound.remove_channel_handler(handler_id));

        let (_sequence, packet) =
            inbound.send_channel_message(0x6161, b"no-consumer".to_vec()).expect("channel message");
        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
        assert!(seen.lock().expect("lock").is_empty());
    }

    #[test]
    fn channel_handler_panics_do_not_unwind_receive_path() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        outbound.register_channel_handler(0x9999, |_| -> bool { panic!("boom") });

        let (_sequence, packet) =
            inbound.send_channel_message(0x9999, b"panic".to_vec()).expect("channel message");

        let result = catch_unwind(AssertUnwindSafe(|| outbound.handle_packet(&packet, iface)));
        assert!(result.is_ok(), "channel handler panic should be contained");
        assert!(matches!(result.unwrap(), LinkHandleResult::Proof(_)));
    }

    #[test]
    fn channel_send_window_limits_outstanding_messages_until_proved() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));
        inbound.register_channel_handler(0x7000, |_| true);

        let (_first_sequence, first_packet) = outbound
            .send_channel_message(0x7000, b"first".to_vec())
            .expect("first channel message");
        let (_second_sequence, _second_packet) = outbound
            .send_channel_message(0x7000, b"second".to_vec())
            .expect("second channel message");
        assert!(matches!(
            outbound.send_channel_message(0x7000, b"third".to_vec()),
            Err(ChannelError::LinkNotReady)
        ));

        let proof = match inbound.handle_packet(&first_packet, iface) {
            LinkHandleResult::Proof(proof) => proof,
            _ => panic!("first channel packet should generate proof"),
        };
        assert!(matches!(outbound.handle_packet(&proof, iface), LinkHandleResult::None));
        assert!(outbound.send_channel_message(0x7000, b"third".to_vec()).is_ok());
    }

    #[test]
    fn slow_rtt_links_start_with_single_channel_slot() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        outbound.rtt = Duration::from_secs_f32(1.6);
        outbound.refresh_channel_flow_control();
        assert!(outbound.send_channel_message(0x7001, b"first".to_vec()).is_ok());
        assert!(matches!(
            outbound.send_channel_message(0x7001, b"second".to_vec()),
            Err(ChannelError::LinkNotReady)
        ));
    }

    #[test]
    fn channel_window_grows_after_successful_deliveries() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));
        inbound.register_channel_handler(0x7200, |_| true);

        assert_eq!(outbound.channel_send_window(), 2);

        let (_sequence, packet) =
            outbound.send_channel_message(0x7200, b"first".to_vec()).expect("channel message");
        let proof = match inbound.handle_packet(&packet, iface) {
            LinkHandleResult::Proof(proof) => proof,
            _ => panic!("channel packet should generate proof"),
        };
        assert!(matches!(outbound.handle_packet(&proof, iface), LinkHandleResult::None));

        assert_eq!(outbound.channel_send_window(), 3);
    }

    #[test]
    fn channel_window_shrinks_after_retry_timeout() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));
        inbound.register_channel_handler(0x7201, |_| true);

        let (_sequence, packet) =
            outbound.send_channel_message(0x7201, b"grow".to_vec()).expect("channel message");
        let proof = match inbound.handle_packet(&packet, iface) {
            LinkHandleResult::Proof(proof) => proof,
            _ => panic!("channel packet should generate proof"),
        };
        assert!(matches!(outbound.handle_packet(&proof, iface), LinkHandleResult::None));
        assert_eq!(outbound.channel_send_window(), 3);

        let (_sequence, _packet) =
            outbound.send_channel_message(0x7201, b"timeout".to_vec()).expect("channel message");
        let _ = outbound.poll_channel_timeouts(Instant::now() + Duration::from_secs(1));

        assert_eq!(outbound.channel_send_window(), 2);
    }

    #[test]
    fn timed_out_channel_messages_are_retransmitted() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let (sequence, packet) =
            outbound.send_channel_message(0x7100, b"retry-me".to_vec()).expect("channel message");
        let resend_packets =
            outbound.poll_channel_timeouts(Instant::now() + Duration::from_secs(1));

        assert_eq!(resend_packets.len(), 1);
        assert_eq!(resend_packets[0].hash(), packet.hash());
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Sent);
        assert_eq!(outbound.status(), LinkStatus::Active);
    }

    #[test]
    fn channel_retry_exhaustion_closes_link_and_fails_pending_messages() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let (sequence, _packet) = outbound
            .send_channel_message(0x7101, b"eventually-fails".to_vec())
            .expect("channel message");

        for seconds in 1..=4 {
            let resend_packets =
                outbound.poll_channel_timeouts(Instant::now() + Duration::from_secs(seconds));
            assert_eq!(resend_packets.len(), 1);
            assert_eq!(outbound.status(), LinkStatus::Active);
            assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Sent);
        }

        let resend_packets =
            outbound.poll_channel_timeouts(Instant::now() + Duration::from_secs(5));
        assert!(resend_packets.is_empty());
        assert_eq!(outbound.status(), LinkStatus::Closed);
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Failed);
    }

    #[test]
    fn channel_messages_mark_delivered_when_their_link_proof_arrives() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));
        inbound.register_channel_handler(0x55AA, |_| true);

        let (sequence, packet) = outbound
            .send_channel_message(0x55AA, b"needs-proof".to_vec())
            .expect("channel message");
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Sent);

        let proof = match inbound.handle_packet(&packet, iface) {
            LinkHandleResult::Proof(proof) => proof,
            _ => panic!("channel packet should generate link proof"),
        };
        assert!(matches!(outbound.handle_packet(&proof, iface), LinkHandleResult::None));
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Delivered);
    }

    #[test]
    fn pending_channel_messages_fail_when_link_closes() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let (sequence, _packet) =
            outbound.send_channel_message(0x9001, b"will-fail".to_vec()).expect("channel message");
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Sent);

        outbound.close();
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Failed);
    }

    #[test]
    fn activity_timers_exclude_keepalives_from_data_timer() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut link = Link::new(destination, tx);
        let stale_anchor = Instant::now() - Duration::from_secs(5);
        link.status = LinkStatus::Active;
        link.activated_at = Some(stale_anchor);
        link.last_inbound = Some(stale_anchor);
        link.last_outbound = Some(stale_anchor);
        link.last_data = Some(stale_anchor);

        link.note_inbound(PacketContext::KeepAlive);
        link.note_outbound(PacketContext::KeepAlive);

        assert!(link.no_inbound_for() < Duration::from_secs(1));
        assert!(link.no_outbound_for() < Duration::from_secs(1));
        assert!(link.no_data_for() >= Duration::from_secs(5));
        assert!(link.inactive_for() < Duration::from_secs(1));
    }

    #[test]
    fn teardown_builds_link_close_packet_and_purges_session_state() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let packet = outbound.teardown().expect("active teardown should produce close packet");
        assert_eq!(packet.header.packet_type, PacketType::Data);
        assert_eq!(packet.context, PacketContext::LinkClose);
        assert_eq!(outbound.status(), LinkStatus::Closed);
        assert!(outbound.session_cipher.is_none());
        assert_eq!(outbound.derived_key.as_bytes(), DerivedKey::new_empty().as_bytes());
        assert_eq!(outbound.peer_identity().address_hash, Identity::default().address_hash);

        let mut plain = [0u8; PACKET_MDU];
        let decrypted = inbound.decrypt(packet.data.as_slice(), &mut plain).expect("decrypt close");
        assert_eq!(decrypted, outbound.id().as_slice());
    }

    #[test]
    fn inbound_link_close_packet_tears_down_active_link() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let close_packet =
            outbound.teardown().expect("active teardown should produce close packet");
        assert!(matches!(inbound.handle_packet(&close_packet, iface), LinkHandleResult::None));
        assert_eq!(inbound.status(), LinkStatus::Closed);
        assert!(inbound.session_cipher.is_none());
        assert_eq!(inbound.derived_key.as_bytes(), DerivedKey::new_empty().as_bytes());
    }

    #[test]
    fn link_rtt_packets_update_adaptive_keepalive_timing() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        outbound.request_time = Instant::now() - Duration::from_millis(250);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let rtt_packet = outbound.create_rtt();
        assert!(matches!(inbound.handle_packet(&rtt_packet, iface), LinkHandleResult::None));
        assert!(outbound.rtt >= Duration::from_millis(250));
        assert!(inbound.rtt > Duration::ZERO);
        assert!(inbound.keepalive < Duration::from_secs_f32(KEEPALIVE_MAX_SECS));
        assert_eq!(
            inbound.stale_time,
            Duration::from_secs_f32(inbound.keepalive.as_secs_f32() * STALE_FACTOR)
        );
    }

    #[test]
    fn next_watchdog_deadline_tracks_keepalive_and_stale_timeouts() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut link = Link::new(destination, tx);
        link.status = LinkStatus::Active;
        link.rtt = Duration::from_millis(200);
        link.update_keepalive_timing();
        let anchor = Instant::now();
        link.activated_at = Some(anchor);
        link.last_inbound = Some(anchor);
        let active_deadline = link.next_watchdog_deadline(true).expect("active deadline");
        assert!(active_deadline >= anchor + link.keepalive);

        link.status = LinkStatus::Stale;
        link.stale_since = Some(anchor);
        let stale_deadline = link.next_watchdog_deadline(false).expect("stale deadline");
        let expected = anchor
            + Duration::from_secs_f32(
                (link.rtt.as_secs_f32() * KEEPALIVE_TIMEOUT_FACTOR) + STALE_GRACE_SECS,
            );
        assert_eq!(stale_deadline, expected);
    }

    #[test]
    fn watchdog_transitions_active_links_to_stale_and_then_closed() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut link = Link::new(destination, tx);
        link.status = LinkStatus::Active;
        link.rtt = Duration::from_millis(500);
        link.update_keepalive_timing();
        link.activated_at = Some(Instant::now() - link.stale_time - Duration::from_secs(1));
        link.last_inbound = link.activated_at;

        assert_eq!(link.check_watchdog(false), LinkWatchdogAction::None);
        assert_eq!(link.status, LinkStatus::Stale);
        assert!(link.stale_since.is_some());

        link.stale_since = Some(
            Instant::now()
                - Duration::from_secs_f32(
                    (link.rtt.as_secs_f32() * KEEPALIVE_TIMEOUT_FACTOR) + STALE_GRACE_SECS + 1.0,
                ),
        );
        let action = link.check_watchdog(false);
        let packet = match action {
            LinkWatchdogAction::SendTeardown(packet) => packet,
            _ => panic!("watchdog should emit teardown packet"),
        };
        assert_eq!(packet.context, PacketContext::LinkClose);
        assert_eq!(link.status, LinkStatus::Closed);
    }

    #[test]
    fn watchdog_requests_keepalive_for_initiator_links() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut link = Link::new(destination, tx);
        link.status = LinkStatus::Active;
        link.rtt = Duration::from_millis(20);
        link.update_keepalive_timing();
        let anchor = Instant::now() - link.keepalive - Duration::from_secs(1);
        link.activated_at = Some(anchor);
        link.last_inbound = Some(anchor);
        link.last_keepalive = Some(anchor);

        assert_eq!(link.check_watchdog(true), LinkWatchdogAction::SendKeepAlive);
        assert_eq!(link.status, LinkStatus::Active);
    }
}
