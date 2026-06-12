use crate::{
    error::RnsError,
    hash::{AddressHash, Hash},
    identity::{EmptyIdentity, HashIdentity, Identity, PrivateIdentity, PUBLIC_KEY_LENGTH},
    packet::{
        self, ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
        PacketDataBuffer, PacketType, PropagationType,
    },
    ratchets::{decrypt_with_identity, now_secs},
};

use core::{fmt, marker::PhantomData};

use ed25519_dalek::{Signature, SigningKey, VerifyingKey, SIGNATURE_LENGTH};

use rand_core::CryptoRngCore;

use sha2::Digest;

use std::collections::{BTreeMap, VecDeque};

use std::path::Path;

use std::time::Instant;

use x25519_dalek::PublicKey;
