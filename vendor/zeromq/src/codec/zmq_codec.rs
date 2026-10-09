use super::command::ZmqCommand;
use super::error::CodecError;
use super::greeting::ZmqGreeting;
use super::Message;
use crate::ZmqMessage;

use asynchronous_codec::{Decoder, Encoder};
use bytes::{Buf, BufMut, Bytes, BytesMut};

use std::convert::TryFrom;

#[derive(Debug, Clone, Copy)]
struct Frame {
    command: bool,
    long: bool,
    more: bool,
}

#[derive(Debug)]
enum DecoderState {
    Greeting,
    FrameHeader,
    FrameLen(Frame),
    Frame(Frame),
}

#[derive(Debug)]
pub struct ZmqCodec {
    state: DecoderState,
    waiting_for: usize, // Number of bytes needed to decode frame
    // Needed to store incoming multipart message
    // This allows to encapsulate its processing inside codec and not expose
    // internal details to higher levels
    buffered_message: Option<ZmqMessage>,
    buffered_bytes: usize,
    charged_bytes: usize,
}

impl ZmqCodec {
    pub fn new() -> Self {
        Self {
            state: DecoderState::Greeting,
            waiting_for: 64, // len of the greeting frame
            buffered_message: None,
            buffered_bytes: 0,
            charged_bytes: 0,
        }
    }
}

// The codec acquires before reserve/reading the declared frame. Errors/drop release all charges.
static RECEIVE_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
impl ZmqCodec {
    fn release_bytes(&mut self) {
        RECEIVE_BYTES.fetch_sub(self.charged_bytes, std::sync::atomic::Ordering::AcqRel);
        self.charged_bytes = 0;
        self.buffered_bytes = 0;
    }
}
impl Drop for ZmqCodec {
    fn drop(&mut self) {
        self.release_bytes();
    }
}

struct ChargedFrame {
    bytes: Bytes,
    charge: usize,
}
impl AsRef<[u8]> for ChargedFrame {
    fn as_ref(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}
impl Drop for ChargedFrame {
    fn drop(&mut self) {
        RECEIVE_BYTES.fetch_sub(self.charge, std::sync::atomic::Ordering::AcqRel);
    }
}
impl Default for ZmqCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for ZmqCodec {
    type Error = CodecError;
    type Item = Message;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < self.waiting_for {
            src.reserve(self.waiting_for - src.len());
            return Ok(None);
        }
        match self.state {
            DecoderState::Greeting => {
                if src[0] != 0xff {
                    return Err(CodecError::Decode("Bad first byte of greeting"));
                }
                self.state = DecoderState::FrameHeader;
                self.waiting_for = 1;
                Ok(Some(Message::Greeting(ZmqGreeting::try_from(
                    src.split_to(64).freeze(),
                )?)))
            }
            DecoderState::FrameHeader => {
                let flags = src.get_u8();

                let frame = Frame {
                    command: (flags & 0b0000_0100) != 0,
                    long: (flags & 0b0000_0010) != 0,
                    more: (flags & 0b0000_0001) != 0,
                };
                if flags & !7 != 0 || (frame.command && self.buffered_message.is_some()) {
                    return Err(CodecError::Decode(
                        "invalid frame flags or interleaved command",
                    ));
                }
                self.state = DecoderState::FrameLen(frame);
                self.waiting_for = if frame.long { 8 } else { 1 };
                self.decode(src)
            }
            DecoderState::FrameLen(frame) => {
                self.state = DecoderState::Frame(frame);
                self.waiting_for = if frame.long {
                    usize::try_from(src.get_u64())
                        .map_err(|_| CodecError::Decode("ZMTP length overflows this platform"))?
                } else {
                    src.get_u8() as usize
                };
                if (frame.command && self.waiting_for > 64 * 1024)
                    || self.waiting_for > (16 * 1024 * 1024 + 65 * 1024)
                    || self.buffered_bytes.saturating_add(self.waiting_for)
                        > (16 * 1024 * 1024 + 65 * 1024)
                    || self
                        .buffered_message
                        .as_ref()
                        .is_some_and(|m| m.len() >= 32)
                {
                    return Err(CodecError::Decode(
                        "ZMTP frame/multipart exceeds bounded RPC resources",
                    ));
                }
                RECEIVE_BYTES
                    .fetch_update(
                        std::sync::atomic::Ordering::AcqRel,
                        std::sync::atomic::Ordering::Acquire,
                        |used| {
                            used.checked_add(self.waiting_for)
                                .filter(|n| *n <= 128 * 1024 * 1024)
                        },
                    )
                    .map_err(|_| CodecError::Decode("ZMTP receive byte capacity exhausted"))?;
                self.charged_bytes += self.waiting_for;
                self.buffered_bytes += self.waiting_for;
                self.decode(src)
            }
            DecoderState::Frame(frame) => {
                let data = src.split_to(self.waiting_for);
                self.state = DecoderState::FrameHeader;
                self.waiting_for = 1;
                if frame.command {
                    self.release_bytes();
                    return Ok(Some(Message::Command(ZmqCommand::try_from(data.freeze())?)));
                }

                // Owned Bytes retains its charge through queueing, clones and frame extraction.
                let charge = data.len();
                let frame_data = Bytes::from_owner(ChargedFrame {
                    bytes: Bytes::copy_from_slice(&data),
                    charge,
                });
                self.charged_bytes -= charge;
                match &mut self.buffered_message {
                    Some(v) => v.push_back(frame_data),
                    None => self.buffered_message = Some(ZmqMessage::from(frame_data)),
                }

                if frame.more {
                    self.decode(src)
                } else {
                    self.release_bytes();
                    // Quoth the Raven “Nevermore.”
                    Ok(Some(Message::Message(
                        self.buffered_message
                            .take()
                            .expect("Corrupted decoder state"),
                    )))
                }
            }
        }
    }
}

fn encode_frame(frame: &Bytes, dst: &mut BytesMut, more: bool) {
    let mut flags: u8 = 0;
    if more {
        flags |= 0b0000_0001;
    }
    let len = frame.len();
    if len > 255 {
        flags |= 0b0000_0010;
        dst.reserve(len + 9);
    } else {
        dst.reserve(len + 2);
    }
    dst.put_u8(flags);
    if len > 255 {
        dst.put_u64(len as u64);
    } else {
        dst.put_u8(len as u8);
    }
    dst.extend_from_slice(frame.as_ref());
}

impl Encoder for ZmqCodec {
    type Error = CodecError;
    type Item<'a> = Message;

    fn encode(&mut self, message: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match message {
            Message::Greeting(payload) => dst.unsplit(payload.into()),
            Message::Command(command) => dst.unsplit(command.into()),
            Message::Message(message) => {
                if message.is_empty()
                    || message.len() > 32
                    || message.iter().map(|b| b.len()).sum::<usize>()
                        > (16 * 1024 * 1024 + 65 * 1024)
                {
                    return Err(CodecError::Other(
                        "outbound multipart exceeds bounded RPC resources",
                    ));
                }
                let last_element = message.len() - 1;
                for (idx, part) in message.iter().enumerate() {
                    encode_frame(part, dst, idx != last_element);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    pub fn test_message_decode_1() {
        let data = "01093c4944537c4d53473e01403239386166316563653932306635373637656132393438376261363164643436613534636334313262653032303339316139653831636535633234383039653001cb7b226d73675f6964223a2236356336396230312d636634622d343563322d616165612d323263306365326531316533222c2273657373696f6e223a2230326462356631642d386535632d346464612d383064342d303337363835343465616138222c22757365726e616d65223a223c544f444f3e222c2264617465223a22323032312d31322d32395430343a35393a33392e3539333533372b30303a3030222c226d73675f74797065223a22657865637574655f7265706c79222c2276657273696f6e223a22352e33227d01c07b226d73675f6964223a223965303336313036373262393433393961343432316539373330333330326162222c2273657373696f6e223a226231323139393364663235613432643839376135653163383362306337616665222c22757365726e616d65223a22757365726e616d65222c2264617465223a22313937302d30312d30315430303a30303a30302b30303a3030222c226d73675f74797065223a22657865637574655f72657175657374222c2276657273696f6e223a22352e32227d01027b7d00467b22737461747573223a226f6b222c22657865637574696f6e5f636f756e74223a312c227061796c6f6164223a5b5d2c22757365725f65787072657373696f6e73223a7b7d7d";
        let hex_data = hex::decode(data).unwrap();
        let mut bytes = BytesMut::from(hex_data.as_slice());
        let mut codec = ZmqCodec::new();
        codec.waiting_for = 1;
        codec.state = DecoderState::FrameHeader;

        let message = codec
            .decode(&mut bytes)
            .expect("decode success")
            .expect("single message");

        eprintln!("{:?}", &message);
        match message {
            Message::Message(m) => {
                assert_eq!(6, m.into_vecdeque().len());
            }
            _ => panic!("wrong message type"),
        }
        assert_eq!(bytes.len(), 0);
    }

    #[test]
    pub fn test_message_decode_2() {
        let data = "01093c4944537c4d53473e01406139346435366530343438353335303831316561623063663730623464356366373933653431653838616330666339646263346562326238616136643635306601cb7b226d73675f6964223a2263383466623933372d333162662d346335622d386430392d386535633230633434333636222c2273657373696f6e223a2230326462356631642d386535632d346464612d383064342d303337363835343465616138222c22757365726e616d65223a223c544f444f3e222c2264617465223a22323032312d31322d32395430343a35393a34332e3037343831332b30303a3030222c226d73675f74797065223a22657865637574655f7265706c79222c2276657273696f6e223a22352e33227d01c07b226d73675f6964223a223238646635316334303933313433643339393131346664333439643530396634222c2273657373696f6e223a226231323139393364663235613432643839376135653163383362306337616665222c22757365726e616d65223a22757365726e616d65222c2264617465223a22313937302d30312d30315430303a30303a30302b30303a3030222c226d73675f74797065223a22657865637574655f72657175657374222c2276657273696f6e223a22352e32227d01027b7d00467b22737461747573223a226f6b222c22657865637574696f6e5f636f756e74223a322c227061796c6f6164223a5b5d2c22757365725f65787072657373696f6e73223a7b7d7d";
        let hex_data = hex::decode(data).unwrap();
        let mut bytes = BytesMut::from(hex_data.as_slice());
        let mut codec = ZmqCodec::new();
        codec.waiting_for = 1;
        codec.state = DecoderState::FrameHeader;

        let message = codec
            .decode(&mut bytes)
            .expect("decode success")
            .expect("single message");
        eprintln!("{:?}", &message);
        assert_eq!(bytes.len(), 0);
        match message {
            Message::Message(m) => {
                assert_eq!(6, m.into_vecdeque().len());
            }
            _ => panic!("wrong message type"),
        }
    }
}

#[cfg(test)]
mod bounded_regressions {
    use super::*;
    fn codec() -> ZmqCodec {
        let mut c = ZmqCodec::new();
        c.state = DecoderState::FrameHeader;
        c.waiting_for = 1;
        c
    }
    #[test]
    fn rejects_declared_lengths_before_reserving_payload() {
        for (flags, length) in [(2, u64::MAX), (2, 17 * 1024 * 1024), (6, 65537)] {
            let mut c = codec();
            let mut bytes = BytesMut::new();
            bytes.put_u8(flags);
            bytes.put_u64(length);
            let before = bytes.capacity();
            assert!(c.decode(&mut bytes).is_err());
            assert!(bytes.capacity() <= before);
        }
    }
    #[test]
    fn frame_charge_survives_clones_and_frame_extraction() {
        let before = RECEIVE_BYTES.load(std::sync::atomic::Ordering::Acquire);
        let mut c = codec();
        let mut bytes = BytesMut::from(&b"\x00\x04test"[..]);
        let Message::Message(message) = c.decode(&mut bytes).unwrap().unwrap() else {
            panic!("message required");
        };
        let clone = message.clone();
        let frames = message.into_vec();
        drop(c);
        assert_eq!(
            RECEIVE_BYTES.load(std::sync::atomic::Ordering::Acquire),
            before + 4
        );
        drop(clone);
        assert_eq!(
            RECEIVE_BYTES.load(std::sync::atomic::Ordering::Acquire),
            before + 4
        );
        drop(frames);
        assert_eq!(
            RECEIVE_BYTES.load(std::sync::atomic::Ordering::Acquire),
            before
        );
    }
    #[test]
    fn cancellation_releases_partial_frame_and_multipart_limits() {
        let before = RECEIVE_BYTES.load(std::sync::atomic::Ordering::Acquire);
        {
            let mut c = codec();
            let mut bytes = BytesMut::from(&b"\x00\x64"[..]);
            assert!(c.decode(&mut bytes).unwrap().is_none());
            assert_eq!(
                RECEIVE_BYTES.load(std::sync::atomic::Ordering::Acquire),
                before + 100
            );
        }
        assert_eq!(
            RECEIVE_BYTES.load(std::sync::atomic::Ordering::Acquire),
            before
        );
        {
            let mut c = codec();
            let mut bytes = BytesMut::from(vec![1, 0].repeat(33).as_slice());
            assert!(c.decode(&mut bytes).is_err());
        }
        assert_eq!(
            RECEIVE_BYTES.load(std::sync::atomic::Ordering::Acquire),
            before
        );
    }
    #[test]
    fn maximum_legal_rpc_frame_roundtrips() {
        let payload = Bytes::from(vec![42; 16 * 1024 * 1024 + 65 * 1024]);
        let mut bytes = BytesMut::new();
        codec()
            .encode(
                Message::Message(ZmqMessage::from(payload.clone())),
                &mut bytes,
            )
            .unwrap();
        let Message::Message(message) = codec().decode(&mut bytes).unwrap().unwrap() else {
            panic!("message required");
        };
        assert_eq!(message.get(0).unwrap(), &payload);
    }
}
