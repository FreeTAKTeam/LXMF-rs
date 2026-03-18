use std::{
    cmp::min,
    time::{Duration, Instant},
};

use ed25519_dalek::{Signature, SigningKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH};
use rand_core::OsRng;
use sha2::Digest;
use x25519_dalek::StaticSecret;

use crate::{
    buffer::OutputBuffer,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkWatchdogAction {
    None,
    SendKeepAlive,
    Close,
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
    last_keepalive: Option<Instant>,
    last_proof: Option<Instant>,
    stale_since: Option<Instant>,
    keepalive: Duration,
    stale_time: Duration,
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
            last_keepalive: None,
            last_proof: None,
            stale_since: None,
            keepalive: Duration::from_secs_f32(KEEPALIVE_MAX_SECS),
            stale_time: Duration::from_secs_f32(KEEPALIVE_MAX_SECS * STALE_FACTOR),
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
            last_keepalive: None,
            last_proof: None,
            stale_since: None,
            keepalive: Duration::from_secs_f32(KEEPALIVE_MAX_SECS),
            stale_time: Duration::from_secs_f32(KEEPALIVE_MAX_SECS * STALE_FACTOR),
            event_tx,
        };

        link.handshake(peer_identity);

        Ok(link)
    }

    pub fn request(&mut self) -> Packet {
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
        self.last_inbound = None;
        self.last_keepalive = None;
        self.last_proof = None;
        self.stale_since = None;
        self.keepalive = Duration::from_secs_f32(KEEPALIVE_MAX_SECS);
        self.stale_time = Duration::from_secs_f32(KEEPALIVE_MAX_SECS * STALE_FACTOR);

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
            PacketContext::None
            | PacketContext::Channel
            | PacketContext::Request
            | PacketContext::Response
            | PacketContext::LinkIdentify => {
                let mut buffer = [0u8; PACKET_MDU];
                if let Ok(plain_text) = self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    let preview_len = plain_text.len().min(32);
                    eprintln!(
                        "[link] data_plain len={} preview={}",
                        plain_text.len(),
                        bytes_to_hex(&plain_text[..preview_len])
                    );
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
                    if matches!(packet.context, PacketContext::None | PacketContext::Channel) {
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
            PacketContext::LinkRTT => {
                let mut buffer = [0u8; PACKET_MDU];
                if let Ok(plain_text) = self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    let mut cursor = std::io::Cursor::new(plain_text);
                    if let Ok(peer_rtt) = rmp::decode::read_f32(&mut cursor) {
                        let measured_rtt = self.request_time.elapsed().as_secs_f32();
                        self.rtt = Duration::from_secs_f32(measured_rtt.max(peer_rtt));
                        self.update_keepalive_timing();
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
            self.request_time = now;
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
                            self.last_keepalive = Some(now);
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
                        self.close();
                        return LinkWatchdogAction::Close;
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
    pub fn close(&mut self) {
        self.status = LinkStatus::Closed;
        self.session_cipher = None;

        self.post_event(LinkEvent::Closed);

        log::warn!("link: close {}", self.id);
    }

    pub fn restart(&mut self) {
        log::warn!("link({}): restart after {}s", self.id, self.request_time.elapsed().as_secs());

        self.status = LinkStatus::Pending;
        self.session_cipher = None;
        self.activated_at = None;
        self.last_inbound = None;
        self.last_keepalive = None;
        self.last_proof = None;
        self.stale_since = None;
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
}

include!("link/proof.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::{DestinationDesc, DestinationName};

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
    fn channel_packets_are_forwarded_and_generate_link_proofs() {
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

        let packet = inbound.channel_packet(b"channel-payload").expect("channel packet");

        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));

        let event = rx.try_recv().expect("channel payload event");
        match event.event {
            LinkEvent::Data(payload) => {
                assert_eq!(payload.context(), PacketContext::Channel);
                assert_eq!(payload.as_slice(), b"channel-payload");
            }
            other => panic!("unexpected event: {:?}", std::mem::discriminant(&other)),
        }
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
        assert_eq!(link.check_watchdog(false), LinkWatchdogAction::Close);
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
