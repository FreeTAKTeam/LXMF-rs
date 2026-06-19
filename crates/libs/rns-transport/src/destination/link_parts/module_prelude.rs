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

const LINK_MTU_MASK: u32 = 0x1F_FFFF;

const LINK_MODE_MASK: u32 = 0xE0_0000;

const RETICULUM_COMPAT_MTU: u32 = (PACKET_MDU + 2 + 1 + ADDRESS_HASH_SIZE * 2) as u32;

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

#[allow(dead_code)]
const CHANNEL_MAX_TRIES: u8 = 5;

#[derive(Debug, Clone)]
struct PendingChannelPacket {
    sequence: u16,
    #[allow(dead_code)]
    packet: Packet,
    #[allow(dead_code)]
    tries: u8,
    #[allow(dead_code)]
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

    fn can_exchange_data(self) -> bool {
        matches!(self, Self::Active)
    }

    #[allow(dead_code)]
    fn can_retry_channel_messages(self) -> bool {
        matches!(self, Self::Active | Self::Stale)
    }

    fn can_send_teardown(self) -> bool {
        matches!(self, Self::Active | Self::Stale)
    }
}

pub type LinkId = AddressHash;
