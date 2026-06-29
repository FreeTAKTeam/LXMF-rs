use alloc::collections::{BTreeMap, VecDeque};

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

type DuplicateKey = (AddressHash, Option<AddressHash>, TagBytes, AddressHash);

type DiscoveryKey = (AddressHash, Option<AddressHash>);

type LocalResponseKey = (AddressHash, Option<AddressHash>, TagBytes, AddressHash);

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
            log::warn!(
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
            let mut raw_requester = [0u8; ADDRESS_HASH_SIZE];
            raw_requester.copy_from_slice(&data[ADDRESS_HASH_SIZE..2 * ADDRESS_HASH_SIZE]);
            requesting_transport = Some(AddressHash::new(raw_requester));
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
    cache: BTreeMap<DuplicateKey, Instant>,
    cache_queue: VecDeque<(DuplicateKey, Instant)>,
    name: String,
    transport_id: Option<AddressHash>,
    controlled_destination: PlainInputDestination,
    discovery: BTreeMap<DiscoveryKey, Instant>,
    pending_recursive_by_iface: BTreeMap<Option<AddressHash>, usize>,
    announce_queue_len: usize,
    announce_cap: usize,
    request_timeout: Duration,
    queue: VecDeque<(DiscoveryKey, Instant)>,
    outgoing_requests: BTreeMap<AddressHash, Instant>,
    outgoing_request_queue: VecDeque<(AddressHash, Instant)>,
    local_response_cache: BTreeMap<LocalResponseKey, Instant>,
    local_response_queue: VecDeque<(LocalResponseKey, Instant)>,
    local_response_cooldown: Duration,
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
            cache: BTreeMap::new(),
            cache_queue: VecDeque::new(),
            name: name.into(),
            transport_id,
            controlled_destination: create_path_request_destination(),
            discovery: BTreeMap::new(),
            pending_recursive_by_iface: BTreeMap::new(),
            announce_queue_len,
            announce_cap,
            request_timeout: Duration::from_secs(request_timeout_secs.max(1)),
            queue: alloc::collections::VecDeque::new(),
            outgoing_requests: BTreeMap::new(),
            outgoing_request_queue: VecDeque::new(),
            local_response_cache: BTreeMap::new(),
            local_response_queue: VecDeque::new(),
            local_response_cooldown: super::LOCAL_PATH_RESPONSE_COOLDOWN,
        }
    }

    fn prune_cache(&mut self, now: Instant) {
        while let Some((key, timeout)) = self.cache_queue.front().cloned() {
            if timeout > now {
                break;
            }
            self.cache_queue.pop_front();
            self.cache.remove(&key);
        }
    }

    fn prune_discovery(&mut self, now: Instant) {
        while let Some((queued_key, timeout)) = self.queue.front().copied() {
            if timeout > now {
                break;
            }
            self.queue.pop_front();
            if self.discovery.remove(&queued_key).is_some() {
                self.decrement_pending_recursive_count(queued_key.1);
            }
        }
    }

    fn prune_local_responses(&mut self, now: Instant) {
        while let Some((key, timeout)) = self.local_response_queue.front().cloned() {
            if timeout > now {
                break;
            }
            self.local_response_queue.pop_front();
            if self.local_response_cache.get(&key).copied() == Some(timeout) {
                self.local_response_cache.remove(&key);
            }
        }
    }

    fn prune_outgoing_requests(&mut self, now: Instant, cooldown: Duration) {
        while let Some((destination, requested_at)) = self.outgoing_request_queue.front().copied() {
            if now.duration_since(requested_at) < cooldown {
                break;
            }
            self.outgoing_request_queue.pop_front();
            if self.outgoing_requests.get(&destination).copied() == Some(requested_at) {
                self.outgoing_requests.remove(&destination);
            }
        }
    }

    pub fn outgoing_request_recently_sent(
        &mut self,
        destination: &AddressHash,
        now: Instant,
        cooldown: Duration,
    ) -> bool {
        self.prune_outgoing_requests(now, cooldown);
        self.outgoing_requests
            .get(destination)
            .map(|requested_at| now.duration_since(*requested_at) < cooldown)
            .unwrap_or(false)
    }

    pub fn record_outgoing_request(&mut self, destination: &AddressHash) {
        self.record_outgoing_request_at(destination, Instant::now());
    }

    fn record_outgoing_request_at(&mut self, destination: &AddressHash, now: Instant) {
        self.outgoing_requests.insert(*destination, now);
        self.outgoing_request_queue.push_back((*destination, now));
    }

    pub fn decode(&mut self, data: &[u8], on_iface: AddressHash) -> Option<PathRequest> {
        self.decode_at(data, on_iface, Instant::now())
    }

    fn decode_at(&mut self, data: &[u8], on_iface: AddressHash, now: Instant) -> Option<PathRequest> {
        let path_request = PathRequest::decode(data, &self.name);
        self.prune_cache(now);

        if let Some(ref request) = path_request {
            let key = (
                request.destination,
                request.requesting_transport,
                request.tag_bytes.clone(),
                on_iface,
            );
            let expires_at = now + self.request_timeout;
            let is_new = self.cache.insert(key.clone(), expires_at).is_none();

            if !is_new {
                log::debug!(
                    "tp({}): ignoring duplicate path request for destination {}",
                    self.name,
                    request.destination
                );
                return None;
            }

            self.cache_queue.push_back((key, expires_at));
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

    pub fn allow_local_response(
        &mut self,
        destination: &AddressHash,
        requesting_transport: Option<AddressHash>,
        tag_bytes: &[u8],
        on_iface: AddressHash,
    ) -> bool {
        self.allow_local_response_at(
            destination,
            requesting_transport,
            tag_bytes,
            on_iface,
            Instant::now(),
        )
    }

    fn allow_local_response_at(
        &mut self,
        destination: &AddressHash,
        requesting_transport: Option<AddressHash>,
        tag_bytes: &[u8],
        on_iface: AddressHash,
        now: Instant,
    ) -> bool {
        self.prune_local_responses(now);

        let key = (*destination, requesting_transport, tag_bytes.to_vec(), on_iface);
        if let Some(timeout) = self.local_response_cache.get(&key) {
            if *timeout > now {
                return false;
            }
            self.local_response_cache.remove(&key);
        }

        let expiry = now + self.local_response_cooldown;
        self.local_response_cache.insert(key.clone(), expiry);
        self.local_response_queue.push_back((key, expiry));
        true
    }

    fn allow_recursive(
        &mut self,
        destination: &AddressHash,
        on_iface: Option<AddressHash>,
    ) -> bool {
        self.allow_recursive_at(destination, on_iface, Instant::now())
    }

    fn allow_recursive_at(
        &mut self,
        destination: &AddressHash,
        on_iface: Option<AddressHash>,
        now: Instant,
    ) -> bool {
        let key = (*destination, on_iface);

        self.prune_discovery(now);

        if let Some(timeout) = self.discovery.get(&key) {
            if *timeout >= now {
                log::debug!(
                    "tp({}): rejecting discovery path request for destination {} on iface {:?} as a request is already pending",
                    self.name,
                    destination,
                    on_iface
                );
                return false;
            }
            self.discovery.remove(&key);
            self.decrement_pending_recursive_count(on_iface);
        }

        let pending_for_iface = self.pending_recursive_count(on_iface);

        if self.announce_cap > 0 && pending_for_iface >= self.announce_cap {
            log::debug!(
                "tp({}): rejecting discovery path request for destination {} on iface {:?} as announce cap reached",
                self.name,
                destination,
                on_iface
            );
            return false;
        }

        if self.announce_queue_len > 0 && pending_for_iface >= self.announce_queue_len {
            log::debug!(
                "tp({}): rejecting discovery path request for destination {} on iface {:?} as announce queue is full",
                self.name,
                destination,
                on_iface
            );
            return false;
        }

        let expiry = now + self.request_timeout;
        self.discovery.insert(key, expiry);
        self.increment_pending_recursive_count(on_iface);
        self.queue.push_back((key, expiry));

        true
    }

    fn pending_recursive_count(&self, on_iface: Option<AddressHash>) -> usize {
        match on_iface {
            Some(iface) => self.pending_recursive_by_iface.get(&Some(iface)).copied().unwrap_or(0),
            None => self.discovery.len(),
        }
    }

    fn increment_pending_recursive_count(&mut self, on_iface: Option<AddressHash>) {
        let count = self.pending_recursive_by_iface.entry(on_iface).or_insert(0);
        *count += 1;
    }

    fn decrement_pending_recursive_count(&mut self, on_iface: Option<AddressHash>) {
        if let Some(count) = self.pending_recursive_by_iface.get_mut(&on_iface) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.pending_recursive_by_iface.remove(&on_iface);
            }
        }
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

    pub fn take_discovery_requesters(&mut self, destination: &AddressHash) -> Vec<AddressHash> {
        self.prune_discovery(Instant::now());

        let keys: Vec<DiscoveryKey> = self
            .discovery
            .keys()
            .copied()
            .filter(|(queued_destination, _)| queued_destination == destination)
            .collect();
        let mut requesters = Vec::new();

        for key in keys {
            if self.discovery.remove(&key).is_some() {
                self.decrement_pending_recursive_count(key.1);
                if let Some(iface) = key.1 {
                    requesters.push(iface);
                }
            }
        }

        requesters
    }
}
