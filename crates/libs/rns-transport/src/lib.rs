//! Transport boundary crate for runtime crates and daemon entrypoints.

pub mod core {
    pub use rns_core::*;
}

pub use legacy_transport::destination::{group_decrypt, group_encrypt};
pub use legacy_transport::error::RnsError;
pub use legacy_transport::hash::lxmf_address_hash;
pub use legacy_transport::identity::{lxmf_sign, lxmf_verify, Identity, PrivateIdentity};
pub use legacy_transport::packet::{Packet, LXMF_MAX_PAYLOAD};
pub use legacy_transport::transport::{DeliveryReceipt, ReceiptHandler};

pub use legacy_transport::{
    buffer, channel, config, crypt, delivery, destination, destination_hash, error, hash, identity,
    iface, packet, ratchets, receipt, resource, storage, time, transport,
};
