pub(super) struct LocalUnpeerCleanup {
    pub(super) peer: String,
    pub(super) removed: bool,
    pub(super) propagation_cleared: usize,
    pub(super) propagation_cleared_bytes: u64,
    pub(super) messages: JsonValue,
}

impl RpcDaemon {
    pub(super) fn unpeer_local_state(
        &self,
        peer_id: &str,
    ) -> Result<LocalUnpeerCleanup, std::io::Error> {
        let peer_id = peer_id.trim();
        let peer_key = self
            .peers
            .lock()
            .expect("peers mutex poisoned")
            .values()
            .find(|record| record.peer.eq_ignore_ascii_case(peer_id))
            .map(|record| record.peer.clone())
            .unwrap_or_else(|| peer_id.to_string());
        let record = {
            let peers = self.peers.lock().expect("peers mutex poisoned");
            peers.get(peer_key.as_str()).cloned()
        };
        if let Some(record) = record {
            self.restore_peer_record_queue_marks(&record)?;
        }
        let propagation_mark_stats = self
            .store
            .peer_propagation_mark_stats(peer_key.as_str())
            .map_err(std::io::Error::other)?;
        let (outgoing, incoming, offered, unhandled, offered_bytes, unhandled_bytes) =
            self.peer_message_stats(peer_key.as_str())?;
        let handled_ids = self
            .store
            .list_peer_handled_propagation_ids(peer_key.as_str())
            .map_err(std::io::Error::other)?;
        let unhandled_ids = self
            .store
            .list_peer_unhandled_propagation_ids(peer_key.as_str())
            .map_err(std::io::Error::other)?;
        self.store
            .clear_peer_propagation_marks(peer_key.as_str())
            .map_err(std::io::Error::other)?;
        let messages = json!({
            "offered": offered,
            "outgoing": outgoing,
            "incoming": incoming,
            "unhandled": unhandled,
            "offered_bytes": offered_bytes,
            "unhandled_bytes": unhandled_bytes,
            "handled_ids": handled_ids,
            "unhandled_ids": unhandled_ids,
        });
        let removed = {
            let mut guard = self.peers.lock().expect("peers mutex poisoned");
            let remove_key =
                guard.keys().find(|key| key.eq_ignore_ascii_case(peer_key.as_str())).cloned();
            let removed = remove_key.and_then(|key| guard.remove(&key)).is_some();
            let peer_count = Self::active_peer_count_from_guard(&guard);
            drop(guard);
            self.update_daemon_status_snapshot(|snapshot| {
                snapshot.peer_count = peer_count;
            });
            removed
        };
        let mut cleared_selected_node = false;
        {
            let mut guard =
                self.outbound_propagation_node.lock().expect("propagation node mutex poisoned");
            if guard.as_deref().is_some_and(|peer| peer.eq_ignore_ascii_case(peer_key.as_str())) {
                *guard = None;
                cleared_selected_node = true;
            }
        }
        if cleared_selected_node {
            let state = {
                let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
                guard.selected_node = None;
                guard.clone()
            };
            self.update_daemon_status_snapshot(|snapshot| {
                snapshot.propagation = state;
            });
        }
        let static_peer_state = {
            let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
            let before = guard.static_peers.len();
            guard.static_peers.retain(|peer| !peer.eq_ignore_ascii_case(peer_key.as_str()));
            (guard.static_peers.len() != before).then(|| guard.clone())
        };
        if let Some(state) = static_peer_state {
            self.update_daemon_status_snapshot(|snapshot| {
                snapshot.propagation = state;
            });
        }
        Ok(LocalUnpeerCleanup {
            peer: peer_key,
            removed,
            propagation_cleared: propagation_mark_stats.entries as usize,
            propagation_cleared_bytes: propagation_mark_stats.bytes,
            messages,
        })
    }
}

pub(super) fn peer_peering_key_value(peer: &PeerRecord, local_identity_hash: &str) -> Option<u32> {
    let peering_cost = peer.peering_cost?;
    if let Some(value) = peer.peering_key_value.filter(|value| *value >= peering_cost) {
        return Some(value);
    }
    let remote_hash = decode_truncated_hash(peer.peer.as_str())?;
    let local_hash = decode_truncated_hash(local_identity_hash)?;
    let mut material = Vec::with_capacity(remote_hash.len() + local_hash.len());
    material.extend_from_slice(remote_hash.as_slice());
    material.extend_from_slice(local_hash.as_slice());
    match generate_peering_key_value(material.as_slice(), peering_cost) {
        Ok(value) => Some(value),
        Err(err) => {
            log::warn!("[rpc] peering key generation failed peer={}: {err}", peer.peer);
            None
        }
    }
}

pub(super) fn peer_peering_key_status(peer: &PeerRecord, peering_key: Option<u32>) -> &'static str {
    match (peer.peering_cost, peering_key) {
        (None, _) => "unconfigured",
        (Some(_), Some(_)) => "ready",
        (Some(_), None) => "not_ready",
    }
}

pub(super) fn peer_acceptance_rate_for_reporting(
    cached_rate: f64,
    outgoing: u64,
    offered: u64,
    alive: bool,
) -> f64 {
    if offered > 0 {
        (outgoing as f64 / offered as f64).max(0.0)
    } else if !alive {
        0.0
    } else {
        cached_rate.max(0.0)
    }
}

fn peer_sync_schedule(peer: &PeerRecord) -> (&'static str, Option<&str>) {
    if peer_sync_backoff_active(now_i64(), peer.next_sync_attempt) {
        ("backoff", Some("backoff"))
    } else if let Some(reason) = peer.sync_schedule_reason.as_deref() {
        (peer_sync_schedule_state(reason), Some(reason))
    } else {
        ("idle", None)
    }
}

fn peer_sync_schedule_state(postpone_reason: &str) -> &'static str {
    if postpone_reason == "backoff" {
        "backoff"
    } else {
        "postponed"
    }
}

fn peer_stamp_policy_known(peer: &PeerRecord) -> bool {
    if peer.propagation_stamp_cost == Some(0) {
        return true;
    }
    peer.propagation_stamp_cost.is_some()
        && peer.propagation_stamp_cost_flexibility.is_some()
        && peer.peering_cost.is_some()
}

fn peer_stamp_policy_partially_known(peer: &PeerRecord) -> bool {
    peer.propagation_stamp_cost.is_some()
        || peer.propagation_stamp_cost_flexibility.is_some()
        || peer.peering_cost.is_some()
}

fn peer_minimum_accepted_stamp_value(peer: &PeerRecord) -> Option<u32> {
    let _cost = peer.propagation_stamp_cost?;
    let _flexibility = peer.propagation_stamp_cost_flexibility?;
    // Python LXMPeer uses min(0, cost - flexibility), so positive stamp values are never rejected here.
    Some(0)
}

fn peer_sync_limits(
    record: &PeerRecord,
    requested_transfer_limit_bytes: Option<usize>,
) -> (Option<usize>, Option<usize>) {
    let record_transfer_limit_bytes = record.propagation_transfer_limit.map(|limit| limit as usize);
    let transfer_limit_bytes = match (record_transfer_limit_bytes, requested_transfer_limit_bytes) {
        (Some(record_limit), Some(requested_limit)) => Some(record_limit.min(requested_limit)),
        (Some(record_limit), None) => Some(record_limit),
        (None, Some(requested_limit)) => Some(requested_limit),
        (None, None) => None,
    };
    let sync_limit_bytes = record.propagation_sync_limit.map(|limit| limit as usize);

    (transfer_limit_bytes, sync_limit_bytes)
}

fn peer_sync_policy_relevance(
    pending_propagation: &[PropagationEntryRecord],
    wanted_ids: Option<&PeerSyncWantedIds>,
    sync_limit_bytes: Option<usize>,
) -> (usize, bool) {
    let mut policy_relevant_pending = 0usize;
    let mut policy_relevant_has_stamp = false;
    let mut policy_relevant_size = 24usize;
    let policy_wanted_ids = wanted_ids.filter(|ids| !ids.wants_none());
    for entry in pending_propagation.iter().filter(|entry| {
        policy_wanted_ids.is_none_or(|ids| ids.wants(entry.transient_id.as_str()))
    }) {
        let entry_size = usize::try_from(entry.size_bytes).unwrap_or(usize::MAX);
        let transfer_size = entry_size.saturating_add(16);
        let next_size = policy_relevant_size.saturating_add(transfer_size);
        if sync_limit_bytes.is_some_and(|limit| next_size >= limit) {
            continue;
        }
        policy_relevant_size = next_size;
        policy_relevant_pending = policy_relevant_pending.saturating_add(1);
        policy_relevant_has_stamp |= entry.stamp_value.is_some();
    }
    (policy_relevant_pending, policy_relevant_has_stamp)
}

pub(super) fn peer_sync_backoff_active(timestamp: i64, next_sync_attempt: i64) -> bool {
    next_sync_attempt > 0 && timestamp <= next_sync_attempt
}

const LXMF_PEER_ERROR_NO_IDENTITY: u8 = 0xf0;

const LXMF_PEER_ERROR_NO_ACCESS: u8 = 0xf1;

const LXMF_PEER_ERROR_INVALID_KEY: u8 = 0xf3;

const LXMF_PEER_ERROR_INVALID_DATA: u8 = 0xf4;

const LXMF_PEER_ERROR_INVALID_STAMP: u8 = 0xf5;

const LXMF_PEER_ERROR_THROTTLED: u8 = 0xf6;

const LXMF_PEER_ERROR_NOT_FOUND: u8 = 0xfd;

const LXMF_PEER_ERROR_TIMEOUT: u8 = 0xfe;

const PN_STAMP_THROTTLE_SECS: i64 = 180;

fn local_retryable_peer_offer_error_reason(offer_error: u8) -> &'static str {
    match offer_error {
        LXMF_PEER_ERROR_NO_IDENTITY => "identity_required",
        LXMF_PEER_ERROR_INVALID_KEY => "invalid_key",
        LXMF_PEER_ERROR_INVALID_DATA => "invalid_data",
        LXMF_PEER_ERROR_INVALID_STAMP => "invalid_stamp",
        LXMF_PEER_ERROR_NOT_FOUND => "not_found",
        LXMF_PEER_ERROR_TIMEOUT => "timeout",
        _ => "peer_offer_error",
    }
}

#[derive(Debug)]
enum PeerSyncWantedIds {
    All,
    Selected(Vec<String>),
}

impl PeerSyncWantedIds {
    fn wants(&self, transient_id: &str) -> bool {
        match self {
            Self::All => true,
            Self::Selected(ids) => ids.iter().any(|id| id == transient_id),
        }
    }

    fn wants_none(&self) -> bool {
        matches!(self, Self::Selected(ids) if ids.is_empty())
    }

    fn requires_offer_validation(&self) -> bool {
        matches!(self, Self::Selected(_))
    }

    fn selected_ids(&self) -> Option<&[String]> {
        match self {
            Self::All => None,
            Self::Selected(ids) => Some(ids.as_slice()),
        }
    }
}

fn canonical_peer_sync_wanted_ids(
    wanted_ids: Option<&JsonValue>,
) -> Result<(Option<PeerSyncWantedIds>, Option<u8>), std::io::Error> {
    let Some(value) = wanted_ids else {
        return Ok((None, None));
    };
    if let Some(error_code) = value.as_u64() {
        let error_code = u8::try_from(error_code).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "wanted_ids error response must fit in one byte",
            )
        })?;
        return Ok((None, Some(error_code)));
    };
    if value.as_bool() == Some(true) {
        return Ok((Some(PeerSyncWantedIds::All), None));
    }
    if value.as_bool() == Some(false) {
        return Ok((Some(PeerSyncWantedIds::Selected(Vec::new())), None));
    }
    let wanted_ids = value.as_array().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "wanted_ids must be true, false, a Python LXMPeer error code, or a list of 32-byte transient ids",
        )
    })?;
    let mut canonical = Vec::with_capacity(wanted_ids.len());
    let mut seen_canonical = std::collections::HashSet::with_capacity(wanted_ids.len());
    for wanted_id in wanted_ids {
        let wanted_id = wanted_id.as_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "wanted_ids must contain 32-byte transient ids",
            )
        })?;
        let wanted_id = wanted_id.trim();
        if wanted_id.len() != 64 || !wanted_id.as_bytes().iter().all(u8::is_ascii_hexdigit) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "wanted_ids must contain 32-byte transient ids",
            ));
        }
        let wanted_id = wanted_id.to_ascii_lowercase();
        if seen_canonical.insert(wanted_id.clone()) {
            canonical.push(wanted_id);
        }
    }
    Ok((Some(PeerSyncWantedIds::Selected(canonical)), None))
}

fn validate_peer_sync_wanted_ids_in_offer(
    wanted_ids: Option<&PeerSyncWantedIds>,
    pending_propagation: &[PropagationEntryRecord],
    transfer_limit_bytes: Option<usize>,
    sync_limit_bytes: Option<usize>,
) -> Result<(), std::io::Error> {
    let Some(wanted_ids) = wanted_ids.and_then(PeerSyncWantedIds::selected_ids) else {
        return Ok(());
    };
    let mut offerable_ids = std::collections::HashSet::with_capacity(pending_propagation.len());
    let mut cumulative_size = 24usize;
    for entry in pending_propagation {
        let entry_size = usize::try_from(entry.size_bytes).unwrap_or(usize::MAX);
        let transfer_size = entry_size.saturating_add(16);
        if transfer_limit_bytes.is_some_and(|limit| transfer_size > limit) {
            continue;
        }
        let next_size = cumulative_size.saturating_add(transfer_size);
        if sync_limit_bytes.is_some_and(|limit| next_size >= limit) {
            continue;
        }
        cumulative_size = next_size;
        offerable_ids.insert(entry.transient_id.as_str());
    }
    for wanted_id in wanted_ids {
        if !offerable_ids.contains(wanted_id.as_str()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "wanted_ids must reference the current peer offer",
            ));
        }
    }
    Ok(())
}

fn validate_peer_sync_full_offer_payloads(
    pending_propagation: &[PropagationEntryRecord],
    transfer_limit_bytes: Option<usize>,
    sync_limit_bytes: Option<usize>,
    start_size: usize,
) -> Result<(), std::io::Error> {
    let mut cumulative_size = start_size;
    for entry in pending_propagation {
        let entry_size = usize::try_from(entry.size_bytes).unwrap_or(usize::MAX);
        let transfer_size = entry_size.saturating_add(16);
        if transfer_limit_bytes.is_some_and(|limit| transfer_size > limit) {
            continue;
        }
        let next_size = cumulative_size.saturating_add(transfer_size);
        if sync_limit_bytes.is_some_and(|limit| next_size >= limit) {
            continue;
        }
        cumulative_size = next_size;
        hex::decode(entry.payload_hex.as_str()).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid propagation payload hex: {err}"),
            )
        })?;
    }
    Ok(())
}

fn peer_sync_resource_data_size(payloads: &[Vec<u8>]) -> Result<u64, std::io::Error> {
    if payloads.is_empty() {
        return Ok(0);
    }
    let packed = rmp_serde::to_vec(&(1.0_f64, payloads)).map_err(std::io::Error::other)?;
    Ok(packed.len() as u64)
}

fn decode_peer_sync_transfer(
    entry: &PropagationEntryRecord,
) -> Result<(JsonValue, Vec<u8>), std::io::Error> {
    let payload_bytes = hex::decode(entry.payload_hex.as_str()).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid propagation payload hex: {err}"),
        )
    })?;
    Ok((
        json!({
            "transient_id": entry.transient_id,
            "destination": entry.destination,
            "payload_hex": entry.payload_hex,
            "received_at": entry.received_at,
            "size_bytes": entry.size_bytes,
            "stamp_value": entry.stamp_value,
        }),
        payload_bytes,
    ))
}

fn propagation_peer_sync_weight(
    entry: &PropagationEntryRecord,
    now: i64,
    prioritised_destinations: &[String],
) -> f64 {
    const FOUR_DAYS_SECS: f64 = 4.0 * 24.0 * 60.0 * 60.0;

    let age_secs = now.saturating_sub(entry.received_at) as f64;
    let age_weight = (age_secs / FOUR_DAYS_SECS).max(1.0);
    let priority_weight = if prioritised_destinations
        .iter()
        .any(|destination| entry.destination.eq_ignore_ascii_case(destination.trim()))
    {
        0.1
    } else {
        1.0
    };
    priority_weight * age_weight * entry.size_bytes as f64
}

fn decode_truncated_hash(value: &str) -> Option<Vec<u8>> {
    let bytes = hex::decode(value.trim()).ok()?;
    (bytes.len() == 16).then_some(bytes)
}

fn generate_peering_key_value(material: &[u8], target_cost: u32) -> Result<u32, &'static str> {
    use hkdf::Hkdf;

    const PEERING_WORKBLOCK_EXPAND_ROUNDS: usize = 25;

    let mut workblock = Vec::with_capacity(PEERING_WORKBLOCK_EXPAND_ROUNDS * 256);
    for n in 0..PEERING_WORKBLOCK_EXPAND_ROUNDS {
        let mut salt_data = Vec::with_capacity(material.len() + 8);
        salt_data.extend_from_slice(material);
        let packed =
            rmp_serde::to_vec(&n).map_err(|_| "failed to encode peering key workblock nonce")?;
        salt_data.extend_from_slice(&packed);
        let salt_hash = Sha256::digest(&salt_data);
        let hk = Hkdf::<Sha256>::new(Some(salt_hash.as_slice()), material);
        let mut okm = [0u8; 256];
        hk.expand(&[], &mut okm)
            .expect("HKDF expand propagation peering key workblock");
        workblock.extend_from_slice(&okm);
    }

    let mut workblock_hasher = Sha256::new();
    workblock_hasher.update(&workblock);
    let mut nonce = 0u64;
    loop {
        let stamp = nonce.to_le_bytes();
        let value = stamp_value_with_prefix(&workblock_hasher, &stamp);
        if value >= target_cost {
            return Ok(value);
        }
        nonce = nonce.wrapping_add(1);
        if nonce == 0 {
            return Err("peering key nonce space exhausted");
        }
    }
}

fn stamp_value_with_prefix(workblock_hasher: &Sha256, stamp: &[u8]) -> u32 {
    let mut hasher = workblock_hasher.clone();
    hasher.update(stamp);
    stamp_value_from_hash(hasher.finalize().as_slice())
}

fn stamp_value_from_hash(hash: &[u8]) -> u32 {
    let mut value = 0u32;
    for byte in hash {
        if *byte == 0 {
            value += 8;
        } else {
            value += byte.leading_zeros();
            break;
        }
    }
    value
}
