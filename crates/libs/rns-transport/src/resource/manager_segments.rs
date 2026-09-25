/// A send that has been fully built but not yet handed to the manager.
///
/// Exists so the expensive half of starting a resource send can happen without
/// the transport handler lock: `ResourceManager::prepare_send` produces one of
/// these from a `&Link` alone, and `track_prepared` files it away. The
/// advertisement packet is already built and encrypted by this point, so
/// filing it needs no link at all.
#[derive(Debug)]
pub struct PreparedSend {
    first: ResourceSender,
    pending: Option<PendingSegments>,
}

/// The not-yet-built tail of an outbound split resource.
///
/// This used to be a `VecDeque<ResourceSender>` filled by `start_send`, which
/// meant one bz2 + AES + SHA-256 pass over the *entire* payload, and chunking
/// it into every fragment it would ever need, before the first advertisement
/// could be returned. `start_send` runs with the transport handler mutex held,
/// so on a 46 MB send that was a full second of global lock before any byte
/// moved (2.5 s once outbound compression was added), and ~100k live `Vec<u8>`
/// fragments retained for the life of the transfer.
///
/// The reference does not do that either: `RNS/Resource.py`'s `advertise()`
/// prepares exactly one segment ahead, on a daemon thread, off the send path.
/// Holding the inputs instead of the outputs is the single-threaded equivalent
/// — segment n+1 is built when segment n's proof arrives, at which point the
/// transfer is idle anyway waiting to advertise it.
///
/// The in-memory variant keeps the historical `Vec<u8>` API working. The
/// reader variant is the bounded-memory path: it retains only the reader and
/// the number of logical bytes still expected, and reads one segment when the
/// preceding segment is proved.
struct PendingSegments {
    link_id: AddressHash,
    source: PendingSegmentSource,
    /// The segment `build_next` will produce, counting from 1. `None` marks
    /// exhaustion, including the case where the final wire index is `u32::MAX`.
    next_segment_index: Option<u32>,
    total_segments: u32,
    total_size: u64,
    request_id: Option<Vec<u8>>,
    is_response: bool,
    interface_mtu: usize,
    original_hash: Hash,
    auto_compress: bool,
}

enum PendingSegmentSource {
    InMemory { data: Vec<u8>, offset: usize },
    Reader { reader: Box<dyn Read + Send + Sync>, remaining: u64 },
}

fn read_resource_reader(reader: &mut dyn Read, length: usize) -> Result<Vec<u8>, RnsError> {
    let mut data = vec![0u8; length];
    reader.read_exact(&mut data).map_err(RnsError::ResourceReader)?;
    Ok(data)
}

impl std::fmt::Debug for PendingSegments {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingSegments")
            .field("link_id", &self.link_id)
            .field("source", &match &self.source {
                PendingSegmentSource::InMemory { .. } => "in_memory",
                PendingSegmentSource::Reader { .. } => "reader",
            })
            .field("next_segment_index", &self.next_segment_index)
            .field("total_segments", &self.total_segments)
            .field("total_size", &self.total_size)
            .field("request_id", &self.request_id)
            .field("is_response", &self.is_response)
            .field("interface_mtu", &self.interface_mtu)
            .field("original_hash", &self.original_hash)
            .field("auto_compress", &self.auto_compress)
            .finish()
    }
}

impl PendingSegments {
    /// Builds the next segment, or `None` once every segment has been built.
    fn build_next(&mut self, link: &Link) -> Option<Result<ResourceSender, RnsError>> {
        let segment_index = self.next_segment_index?;
        let data = match &mut self.source {
            PendingSegmentSource::InMemory { data, offset } => {
                let end = offset.saturating_add(MAX_EFFICIENT_SIZE).min(data.len());
                let chunk = data[*offset..end].to_vec();
                *offset = end;
                chunk
            }
            PendingSegmentSource::Reader { reader, remaining } => {
                let chunk_len = (*remaining).min(MAX_EFFICIENT_SIZE as u64) as usize;
                match read_resource_reader(reader.as_mut(), chunk_len) {
                    Ok(chunk) => {
                        *remaining = remaining.saturating_sub(chunk_len as u64);
                        chunk
                    }
                    Err(error) => return Some(Err(error)),
                }
            }
        };
        let sender = ResourceSender::new_segment_with_options_mtu_and_compression(
            link,
            data,
            None,
            self.request_id.clone(),
            self.is_response,
            self.interface_mtu,
            Some(self.original_hash),
            segment_index,
            self.total_segments,
            Some(self.total_size),
            self.auto_compress,
        );
        if sender.is_ok() {
            self.next_segment_index = if segment_index == self.total_segments {
                None
            } else {
                segment_index.checked_add(1)
            };
        }
        Some(sender)
    }
}

#[derive(Debug)]
struct InboundSegmentAssembly {
    link_id: AddressHash,
    next_segment: u32,
    total_segments: u32,
    total_data_size: u64,
    data: Vec<u8>,
    metadata: Option<Vec<u8>>,
    request_id: Option<Vec<u8>>,
    is_request: bool,
    is_response: bool,
}

impl ResourceManager {
    /// Abandon a partially assembled split resource and tell the caller why.
    ///
    /// Every path that drops an assembly has to come through here. Returning
    /// quietly instead leaves whoever is awaiting the resource blocked until
    /// its own timeout expires, with nothing on either side saying the transfer
    /// is already dead (issue #369).
    fn fail_inbound_segments(&mut self, original_hash: Hash, reason: &str) -> bool {
        let Some(assembly) = self.incoming_segments.remove(&original_hash) else {
            return false;
        };
        log::warn!("split resource assembly failed hash={original_hash} reason={reason}");
        self.events.push(ResourceEvent {
            hash: original_hash,
            link_id: assembly.link_id,
            kind: ResourceEventKind::InboundFailed(ResourceFailure {
                reason: reason.to_string(),
                progress: ResourceProgress {
                    received_bytes: assembly.data.len() as u64,
                    total_bytes: assembly.total_data_size,
                    received_parts: assembly.next_segment.saturating_sub(1) as usize,
                    total_parts: assembly.total_segments as usize,
                },
            }),
        });
        true
    }

    pub fn confirm_outbound_dispatch(&mut self, resource_hash: Hash, sent: bool) {
        let Some(mut sender) = self.pending_outgoing.remove(&resource_hash) else {
            return;
        };

        if sent {
            sender.mark_advertised(self.retry_limit);
            self.outgoing.insert(resource_hash, sender);
        } else {
            self.outgoing_segment_chains.remove(&sender.original_hash);
            self.events.push(ResourceEvent {
                hash: resource_hash,
                link_id: sender.link_id,
                kind: ResourceEventKind::OutboundFailed,
            });
        }
    }

    fn finish_inbound_payload(
        &mut self,
        segment_hash: Hash,
        link_id: AddressHash,
        payload: ResourcePayload,
    ) {
        let segment = payload.segmentation;
        if segment.total_segments == 1 {
            self.events.push(ResourceEvent {
                hash: segment_hash,
                link_id,
                kind: ResourceEventKind::Complete(ResourceComplete {
                    data: payload.data,
                    metadata: payload.metadata,
                    request_id: payload.request_id,
                    is_request: payload.is_request,
                    is_response: payload.is_response,
                }),
            });
            return;
        }

        let assembly = self
            .incoming_segments
            .entry(segment.original_hash)
            .or_insert_with(|| InboundSegmentAssembly {
                link_id,
                next_segment: 1,
                total_segments: segment.total_segments,
                total_data_size: segment.total_data_size,
                data: Vec::new(),
                metadata: payload.metadata.clone(),
                request_id: payload.request_id.clone(),
                is_request: payload.is_request,
                is_response: payload.is_response,
            });
        // Deliberately a drop rather than a failure: issue #520 established that
        // an out-of-order segment must leave the assembly intact so the transfer
        // resumes when the expected segment arrives.
        if assembly.link_id != link_id
            || assembly.total_segments != segment.total_segments
            || assembly.total_data_size != segment.total_data_size
            || assembly.next_segment != segment.segment_index
        {
            log::warn!(
                "discarding inconsistent resource segment original_hash={} segment={}/{}",
                segment.original_hash,
                segment.segment_index,
                segment.total_segments
            );
            return;
        }
        let metadata_size = assembly
            .metadata
            .as_ref()
            .map(|metadata| metadata.len().saturating_add(3))
            .unwrap_or(0);
        if assembly
            .data
            .len()
            .saturating_add(payload.data.len())
            .saturating_add(metadata_size)
            > assembly.total_data_size as usize
        {
            self.fail_inbound_segments(segment.original_hash, "oversized_segment");
            return;
        }
        assembly.data.extend_from_slice(&payload.data);
        assembly.next_segment = assembly.next_segment.saturating_add(1);
        self.events.push(ResourceEvent {
            hash: segment.original_hash,
            link_id,
            kind: ResourceEventKind::SegmentComplete(segment),
        });

        if segment.segment_index == segment.total_segments {
            let Some(assembly) = self.incoming_segments.get(&segment.original_hash) else {
                return;
            };
            let assembled_size = assembly.data.len().saturating_add(
                assembly.metadata.as_ref().map(|metadata| metadata.len() + 3).unwrap_or(0),
            ) as u64;
            if assembled_size != assembly.total_data_size {
                self.fail_inbound_segments(segment.original_hash, "assembled_size_mismatch");
                return;
            }
            if let Some(assembly) = self.incoming_segments.remove(&segment.original_hash) {
                self.events.push(ResourceEvent {
                    hash: segment.original_hash,
                    link_id,
                    kind: ResourceEventKind::Complete(ResourceComplete {
                        data: assembly.data,
                        metadata: assembly.metadata,
                        request_id: assembly.request_id,
                        is_request: assembly.is_request,
                        is_response: assembly.is_response,
                    }),
                });
            }
        }
    }
}
