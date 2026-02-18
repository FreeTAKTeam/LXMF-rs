use alloc::collections::{BTreeMap, BTreeSet, VecDeque};

use rand_core::OsRng;

use tokio::time::{Duration, Instant};

use crate::destination::DestinationName;
use crate::destination::PlainInputDestination;
use crate::hash::AddressHash;
use crate::hash::ADDRESS_HASH_SIZE;
use crate::identity::EmptyIdentity;
use crate::packet::ContextFlag;
use crate::packet::DestinationType;
use crate::packet::Header;
use crate::packet::HeaderType;
use crate::packet::IfacFlag;
use crate::packet::Packet;
use crate::packet::PacketContext;
use crate::packet::PacketDataBuffer;
use crate::packet::PacketType;
use crate::packet::PropagationType;

pub fn create_path_request_destination() -> PlainInputDestination {
    PlainInputDestination::new(
        EmptyIdentity {},
        DestinationName::new("rnstransport", "path.request"),
    )
}

pub type TagBytes = Vec<u8>;

pub fn create_random_tag() -> TagBytes {
    AddressHash::new_from_rand(OsRng).as_slice().into()
}

pub struct PathRequest {
    pub destination: AddressHash,
    pub requesting_transport: Option<AddressHash>,
    pub tag_bytes: TagBytes,
}

impl PathRequest {
    fn decode(data: &[u8], transport_name: &str) -> Option<Self> {
        if data.len() <= ADDRESS_HASH_SIZE {
            log::info!(
                "tp({}): ignoring malformed path request: no {}",
                transport_name,
                if data.len() < ADDRESS_HASH_SIZE { "destination" } else { "tag" }
            );
            return None;
        }

        let mut destination = [0u8; ADDRESS_HASH_SIZE];
        destination.copy_from_slice(&data[..ADDRESS_HASH_SIZE]);
        let destination = AddressHash::new(destination);

        let mut requesting_transport = None;
        let mut tag_start = ADDRESS_HASH_SIZE;
        let mut tag_end = data.len();

        if data.len() > ADDRESS_HASH_SIZE * 2 {
            requesting_transport =
                Some(AddressHash::new_from_slice(&data[ADDRESS_HASH_SIZE..2 * ADDRESS_HASH_SIZE]));
            tag_start = ADDRESS_HASH_SIZE * 2;
        }

        if tag_end - tag_start > ADDRESS_HASH_SIZE {
            tag_end = tag_start + ADDRESS_HASH_SIZE;
        }

        let tag_bytes = data[tag_start..tag_end].into();

        Some(Self { destination, requesting_transport, tag_bytes })
    }
}

pub struct PathRequests {
    cache: BTreeSet<(AddressHash, TagBytes)>,
    name: String,
    transport_id: Option<AddressHash>,
    controlled_destination: PlainInputDestination,
    discovery: BTreeMap<AddressHash, Instant>,
    announce_queue_len: usize,
    announce_cap: usize,
    request_timeout: Duration,
    queue: VecDeque<(AddressHash, Instant)>,
}

impl PathRequests {
    pub fn new(
        name: &str,
        transport_id: Option<AddressHash>,
        announce_queue_len: usize,
        announce_cap: usize,
        request_timeout_secs: u64,
    ) -> Self {
        Self {
            cache: BTreeSet::new(),
            name: name.into(),
            transport_id,
            controlled_destination: create_path_request_destination(),
            discovery: BTreeMap::new(),
            announce_queue_len,
            announce_cap,
            request_timeout: Duration::from_secs(request_timeout_secs.max(1)),
            queue: alloc::collections::VecDeque::new(),
        }
    }

    pub fn decode(&mut self, data: &[u8]) -> Option<PathRequest> {
        let path_request = PathRequest::decode(data, &self.name);

        if let Some(ref request) = path_request {
            let is_new = self.cache.insert((request.destination, request.tag_bytes.clone()));

            if !is_new {
                log::info!(
                    "tp({}): ignoring duplicate path request for destination {}",
                    self.name,
                    request.destination
                );
                return None;
            }
        }

        path_request
    }

    pub fn generate(&mut self, destination: &AddressHash, tag: Option<TagBytes>) -> Packet {
        let mut data = PacketDataBuffer::new_from_slice(destination.as_slice());

        if let Some(transport_id) = self.transport_id {
            data.safe_write(transport_id.as_slice());
        }

        data.safe_write(tag.unwrap_or_else(create_random_tag).as_slice());

        let destination = self.controlled_destination.desc.address_hash;

        Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Plain,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination,
            transport: self.transport_id,
            context: PacketContext::None,
            data,
        }
    }

    fn allow_recursive(
        &mut self,
        destination: &AddressHash,
        _on_iface: Option<AddressHash>,
    ) -> bool {
        let now = Instant::now();

        self.discovery.retain(|_, timeout| *timeout > now);
        while let Some((queued_dest, timeout)) = self.queue.front().copied() {
            if timeout > now {
                break;
            }
            self.queue.pop_front();
            self.discovery.remove(&queued_dest);
        }

        if let Some(timeout) = self.discovery.get(destination) {
            if *timeout >= now {
                log::info!(
                    "tp({}): rejecting discovery path request for destination {} as a request is already pending",
                    self.name,
                    destination
                );
                return false;
            }
            self.discovery.remove(destination);
        }

        if self.announce_cap > 0 && self.discovery.len() >= self.announce_cap {
            log::info!(
                "tp({}): rejecting discovery path request for destination {} as announce cap reached",
                self.name,
                destination
            );
            return false;
        }

        if self.announce_queue_len > 0 && self.queue.len() >= self.announce_queue_len {
            log::info!(
                "tp({}): rejecting discovery path request for destination {} as announce queue is full",
                self.name,
                destination
            );
            return false;
        }

        let expiry = now + self.request_timeout;
        self.discovery.insert(*destination, expiry);
        self.queue.push_back((*destination, expiry));

        true
    }

    pub fn generate_recursive(
        &mut self,
        destination: &AddressHash,
        on_iface: Option<AddressHash>,
        tag: Option<TagBytes>,
    ) -> Option<Packet> {
        if self.allow_recursive(destination, on_iface) {
            log::trace!("tp({}): sending discovery path request for {}", self.name, destination);

            Some(self.generate(destination, tag))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_request_roundtrip() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);

        let dest = AddressHash::new_from_rand(OsRng);

        let encoded = testee.generate(&dest, None);
        let decoded = testee.decode(encoded.data.as_slice()).unwrap();

        assert_eq!(decoded.destination, dest);
    }
}
