use std::sync::{Arc, Mutex};
use std::time::Duration;

use reticulum::channel::{Channel, ChannelOutlet, Envelope};
use reticulum::hash::Hash;
use reticulum::resource::{ResourceAdvertisement, ResourceRequest, RANDOM_HASH_SIZE};

#[test]
fn resource_advertisement_roundtrip() {
    let hash = Hash::new_from_slice(b"resource-hash");
    let mut random_hash = [0u8; RANDOM_HASH_SIZE];
    random_hash.copy_from_slice(&[1, 2, 3, 4]);
    let adv = ResourceAdvertisement {
        transfer_size: 128,
        data_size: 64,
        parts: 2,
        hash,
        random_hash,
        original_hash: hash,
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0x01,
        hashmap: vec![0xAA, 0xBB, 0xCC, 0xDD, 0x01, 0x02, 0x03, 0x04],
    };

    let packed = adv.pack().expect("pack");
    let decoded = ResourceAdvertisement::unpack(&packed).expect("unpack");

    assert_eq!(decoded.transfer_size, adv.transfer_size);
    assert_eq!(decoded.data_size, adv.data_size);
    assert_eq!(decoded.parts, adv.parts);
    assert_eq!(decoded.hash, adv.hash);
    assert_eq!(decoded.random_hash, adv.random_hash);
    assert_eq!(decoded.hashmap, adv.hashmap);
}

#[test]
fn resource_request_roundtrip() {
    let hash = Hash::new_from_slice(b"resource-hash");
    let req = ResourceRequest {
        hashmap_exhausted: true,
        last_map_hash: Some([9, 8, 7, 6]),
        resource_hash: hash,
        requested_hashes: vec![[1, 2, 3, 4], [5, 6, 7, 8]],
    };

    let encoded = req.encode();
    let decoded = ResourceRequest::decode(&encoded).expect("decode");

    assert_eq!(decoded.resource_hash, req.resource_hash);
    assert_eq!(decoded.hashmap_exhausted, req.hashmap_exhausted);
    assert_eq!(decoded.last_map_hash, req.last_map_hash);
    assert_eq!(decoded.requested_hashes.len(), 2);
    assert_eq!(decoded.requested_hashes[0], req.requested_hashes[0]);
    assert_eq!(decoded.requested_hashes[1], req.requested_hashes[1]);
}

struct DummyOutlet {
    pub sent: Vec<Vec<u8>>,
    pub mdu: usize,
}

impl ChannelOutlet for DummyOutlet {
    fn send(&mut self, raw: &[u8]) -> Result<(), reticulum::channel::ChannelError> {
        self.sent.push(raw.to_vec());
        Ok(())
    }

    fn resend(&mut self, raw: &[u8]) -> Result<(), reticulum::channel::ChannelError> {
        self.sent.push(raw.to_vec());
        Ok(())
    }

    fn mdu(&self) -> usize {
        self.mdu
    }

    fn rtt(&self) -> Duration {
        Duration::from_millis(100)
    }

    fn is_usable(&self) -> bool {
        true
    }
}

#[test]
fn channel_envelope_roundtrip() {
    let env = Envelope { msg_type: 0x1001, sequence: 42, payload: b"hello".to_vec() };
    let packed = env.pack();
    let decoded = Envelope::unpack(&packed).expect("unpack");
    assert_eq!(decoded.msg_type, env.msg_type);
    assert_eq!(decoded.sequence, env.sequence);
    assert_eq!(decoded.payload, env.payload);
}

#[test]
fn channel_send_and_receive() {
    let outlet = DummyOutlet { sent: Vec::new(), mdu: 256 };
    let mut channel = Channel::new(outlet);
    let received: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let received_ref = received.clone();
    channel.register_handler(0x2001, move |env| {
        if let Ok(mut guard) = received_ref.lock() {
            *guard = Some(env.payload);
        }
        true
    });

    let seq = channel.send(0x2001, b"ping".to_vec()).expect("send");
    let raw = channel.outlet().sent.last().expect("sent payload").clone();

    channel.receive(&raw).expect("receive");
    assert_eq!(channel.state(seq), reticulum::channel::MessageState::Sent);
    let guard = received.lock().expect("lock");
    assert_eq!(guard.as_ref().unwrap(), b"ping");
    assert_eq!(raw[0..2], 0x2001u16.to_be_bytes());
    assert_eq!(raw[2..4], seq.to_be_bytes());
    assert_eq!(raw[4..6], (4u16).to_be_bytes());
    assert_eq!(&raw[6..], b"ping");
}
