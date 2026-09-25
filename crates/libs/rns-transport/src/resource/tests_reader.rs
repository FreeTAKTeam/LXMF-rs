use std::io::{self, Read};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct CountingReader {
    data: Vec<u8>,
    offset: usize,
    max_read: usize,
    bytes_read: Arc<AtomicUsize>,
}

impl CountingReader {
    fn new(data: Vec<u8>, bytes_read: Arc<AtomicUsize>, max_read: usize) -> Self {
        Self { data, offset: 0, max_read, bytes_read }
    }
}

impl Read for CountingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.offset == self.data.len() {
            return Ok(0);
        }
        let length = (self.data.len() - self.offset).min(buffer.len()).min(self.max_read);
        buffer[..length].copy_from_slice(&self.data[self.offset..self.offset + length]);
        self.offset += length;
        self.bytes_read.fetch_add(length, Ordering::SeqCst);
        Ok(length)
    }
}

struct FailingReader {
    data: Vec<u8>,
    offset: usize,
    fail_at: usize,
}

impl Read for FailingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.offset >= self.fail_at {
            return Err(io::Error::other("synthetic resource reader failure"));
        }
        let length = (self.fail_at - self.offset).min(buffer.len());
        buffer[..length].copy_from_slice(&self.data[self.offset..self.offset + length]);
        self.offset += length;
        Ok(length)
    }
}

struct GeneratedReader {
    remaining: u64,
    byte: u8,
    bytes_read: Arc<AtomicUsize>,
}

impl Read for GeneratedReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = buffer.len().min(self.remaining as usize);
        buffer[..length].fill(self.byte);
        self.remaining -= length as u64;
        self.bytes_read.fetch_add(length, Ordering::SeqCst);
        Ok(length)
    }
}

#[test]
fn reader_backed_send_admits_one_byte_above_compression_threshold_lazily() {
    const COMPRESSION_THRESHOLD: u64 = 64 * 1024 * 1024;
    let (sender_link, _) = resource_link_pair();
    let data_size = COMPRESSION_THRESHOLD + 1;
    let bytes_read = Arc::new(AtomicUsize::new(0));
    let reader = GeneratedReader {
        remaining: data_size,
        byte: 0,
        bytes_read: bytes_read.clone(),
    };
    let mut sender = ResourceManager::new_with_config(Duration::from_secs(30), 8);

    let (original_hash, advertisement_packet) = sender
        .start_send_from_reader(&sender_link, reader, data_size, None)
        .expect("known-size source above the compression threshold is admitted");

    let advertisement = decrypt_advertisement(&sender_link, &advertisement_packet);
    assert_eq!(advertisement.data_size, data_size);
    assert_eq!(advertisement.total_segments, 65);
    assert!(!advertisement.compressed());
    assert_eq!(bytes_read.load(Ordering::SeqCst), MAX_EFFICIENT_SIZE);
    assert!(matches!(
        sender.outgoing_segment_chains.get(&original_hash).map(|pending| &pending.source),
        Some(PendingSegmentSource::Reader { remaining, .. })
            if *remaining == data_size - MAX_EFFICIENT_SIZE as u64
    ));
}

#[test]
fn reader_backed_send_rejects_segment_counts_outside_supported_representation() {
    let (sender_link, _) = resource_link_pair();
    let too_many_segments = (u32::MAX as u64)
        .checked_mul(MAX_EFFICIENT_SIZE as u64)
        .and_then(|size| size.checked_add(1))
        .expect("test size fits u64");
    let result = ResourceManager::prepare_send_from_reader(
        &sender_link,
        io::empty(),
        too_many_segments,
        None,
        None,
        false,
        DEFAULT_RESOURCE_INTERFACE_MTU,
        true,
    );
    assert!(matches!(result, Err(crate::error::RnsError::InvalidArgument)));
}

/// Reader-backed split sends must read only the first segment before the
/// advertisement is tracked, then read one segment at each proof boundary.
#[test]
fn reader_backed_split_send_reassembles_without_retaining_the_full_source() {
    let (mut sender_link, mut receiver_link) = resource_link_pair();
    let payload: Vec<u8> = (0..(MAX_EFFICIENT_SIZE * 2 + 257))
        .map(|index| ((index * 41 + 3) % 251) as u8)
        .collect();
    let bytes_read = Arc::new(AtomicUsize::new(0));
    let reader = CountingReader::new(payload.clone(), bytes_read.clone(), 4096);
    let mut sender = ResourceManager::new_with_config(Duration::from_secs(30), 8);
    let mut receiver = ResourceManager::new_with_config(Duration::from_secs(30), 8);

    let (original_hash, advertisement) = sender
        .start_send_from_reader(&sender_link, reader, payload.len() as u64, None)
        .expect("start reader-backed resource");
    assert_eq!(bytes_read.load(Ordering::SeqCst), MAX_EFFICIENT_SIZE);
    assert!(matches!(
        sender.outgoing_segment_chains.get(&original_hash).map(|pending| &pending.source),
        Some(PendingSegmentSource::Reader { remaining, .. })
            if *remaining == (payload.len() - MAX_EFFICIENT_SIZE) as u64
    ));
    sender.confirm_outbound_dispatch(original_hash, true);

    let mut to_receiver = vec![advertisement];
    let mut delivered = None;
    for _ in 0..100_000 {
        if to_receiver.is_empty() {
            break;
        }
        let mut to_sender = Vec::new();
        for packet in std::mem::take(&mut to_receiver) {
            let plain = decrypt_link_packet(&receiver_link, &packet);
            to_sender.extend(receiver.handle_packet(&plain, &mut receiver_link));
        }
        for event in receiver.drain_events() {
            if let ResourceEventKind::Complete(complete) = event.kind {
                delivered = Some(complete.data);
            }
        }
        for packet in to_sender {
            let plain = decrypt_link_packet(&sender_link, &packet);
            to_receiver.extend(sender.handle_packet(&plain, &mut sender_link));
        }
        sender.drain_events();
    }

    assert_eq!(delivered.as_deref(), Some(payload.as_slice()));
    assert_eq!(bytes_read.load(Ordering::SeqCst), payload.len());
    assert!(sender.outgoing_segment_chains.is_empty());
    assert!(sender.has_no_outbound_state());
}

/// A reader failure while building a later segment must terminate the
/// transfer instead of leaving the sender and receiver waiting forever.
#[test]
fn reader_failure_reports_outbound_resource_failure() {
    let (sender_link, _) = resource_link_pair();
    let data = vec![0x6d; MAX_EFFICIENT_SIZE + 1];
    let mut sender = ResourceManager::new_with_config(Duration::from_secs(1), 2);
    let (original_hash, _) = sender
        .start_send_from_reader(
            &sender_link,
            FailingReader { data: data.clone(), offset: 0, fail_at: MAX_EFFICIENT_SIZE },
            data.len() as u64,
            None,
        )
        .expect("first reader segment should be available");
    sender.confirm_outbound_dispatch(original_hash, true);

    let expected_proof = sender.outgoing.get(&original_hash).expect("first sender").expected_proof;
    let proof = ResourceProof { resource_hash: original_hash, proof: expected_proof };
    let mut link = sender_link;
    let packets = sender.handle_packet(
        &resource_packet(PacketContext::ResourceProof, &proof.encode(), *link.id()),
        &mut link,
    );

    assert!(packets.is_empty());
    assert!(sender.outgoing_segment_chains.is_empty());
    assert!(sender
        .drain_events()
        .iter()
        .any(|event| event.hash == original_hash
            && matches!(event.kind, ResourceEventKind::OutboundFailed)));
}
