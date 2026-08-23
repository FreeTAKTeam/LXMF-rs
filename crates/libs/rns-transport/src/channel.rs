use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageState {
    New,
    Sent,
    Delivered,
    Failed,
}

#[derive(Debug)]
pub enum ChannelError {
    NoHandler,
    LinkNotReady,
    PayloadTooLarge,
    InvalidFrame,
    InvalidMessageType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandlerId(u64);

impl HandlerId {
    pub(crate) fn new(raw: u64) -> Self {
        Self(raw)
    }
}

pub trait ChannelOutlet: Send {
    fn send(&mut self, raw: &[u8]) -> Result<(), ChannelError>;
    fn resend(&mut self, raw: &[u8]) -> Result<(), ChannelError>;
    fn mdu(&self) -> usize;
    fn rtt(&self) -> Duration;
    fn is_usable(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub msg_type: u16,
    pub sequence: u16,
    pub payload: Vec<u8>,
}

impl Envelope {
    pub fn pack(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.payload.len());
        out.extend_from_slice(&self.msg_type.to_be_bytes());
        out.extend_from_slice(&self.sequence.to_be_bytes());
        out.extend_from_slice(&(self.payload.len() as u16).to_be_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    pub fn unpack(raw: &[u8]) -> Result<Self, ChannelError> {
        if raw.len() < 6 {
            return Err(ChannelError::InvalidFrame);
        }
        let msg_type = u16::from_be_bytes([raw[0], raw[1]]);
        let sequence = u16::from_be_bytes([raw[2], raw[3]]);
        let len = u16::from_be_bytes([raw[4], raw[5]]) as usize;
        if raw.len() < 6 + len {
            return Err(ChannelError::InvalidFrame);
        }
        Ok(Self { msg_type, sequence, payload: raw[6..6 + len].to_vec() })
    }
}

pub type Handler = Box<dyn FnMut(Envelope) -> bool + Send>;

pub const SYSTEM_MSG_TYPE_MIN: u16 = 0xF000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum SystemMessageTypes {
    StreamData = 0xFF00,
}

struct RegisteredHandler {
    id: HandlerId,
    handler: Handler,
}

pub trait TypedMessage: Sized {
    const MSG_TYPE: u16;

    fn is_system_type() -> bool {
        false
    }

    fn encode(&self) -> Vec<u8>;

    fn decode(payload: &[u8]) -> Result<Self, ChannelError>;
}

pub(crate) fn validate_typed_message_type<M: TypedMessage>() -> Result<(), ChannelError> {
    if M::MSG_TYPE >= SYSTEM_MSG_TYPE_MIN && !M::is_system_type() {
        return Err(ChannelError::InvalidMessageType);
    }

    Ok(())
}

pub struct Channel<O: ChannelOutlet> {
    outlet: O,
    next_sequence: u16,
    next_rx_sequence: u16,
    next_handler_id: u64,
    handlers: HashMap<u16, Vec<RegisteredHandler>>,
    pending: HashMap<u16, Envelope>,
    states: HashMap<u16, MessageState>,
    rx_ring: HashMap<u16, Envelope>,
}

impl<O: ChannelOutlet> Channel<O> {
    pub fn new(outlet: O) -> Self {
        Self {
            outlet,
            next_sequence: 0,
            next_rx_sequence: 0,
            next_handler_id: 0,
            handlers: HashMap::new(),
            pending: HashMap::new(),
            states: HashMap::new(),
            rx_ring: HashMap::new(),
        }
    }

    pub fn register_handler<F>(&mut self, msg_type: u16, handler: F) -> HandlerId
    where
        F: FnMut(Envelope) -> bool + Send + 'static,
    {
        let id = HandlerId::new(self.next_handler_id);
        self.next_handler_id = self.next_handler_id.wrapping_add(1);
        self.handlers
            .entry(msg_type)
            .or_default()
            .push(RegisteredHandler { id, handler: Box::new(handler) });
        id
    }

    pub fn register_typed_handler<M, F>(
        &mut self,
        mut handler: F,
    ) -> Result<HandlerId, ChannelError>
    where
        M: TypedMessage,
        F: FnMut(M) -> bool + Send + 'static,
    {
        validate_typed_message_type::<M>()?;
        Ok(self.register_handler(M::MSG_TYPE, move |envelope| match M::decode(&envelope.payload) {
            Ok(message) => handler(message),
            Err(_) => false,
        }))
    }

    pub fn remove_handler(&mut self, handler_id: HandlerId) -> bool {
        let mut empty_msg_types = Vec::new();
        let mut removed = false;

        for (msg_type, handlers) in &mut self.handlers {
            let before = handlers.len();
            handlers.retain(|registered| registered.id != handler_id);
            if handlers.is_empty() {
                empty_msg_types.push(*msg_type);
            }
            if handlers.len() != before {
                removed = true;
            }
        }

        for msg_type in empty_msg_types {
            self.handlers.remove(&msg_type);
        }

        removed
    }

    pub fn send(&mut self, msg_type: u16, payload: Vec<u8>) -> Result<u16, ChannelError> {
        if payload.len() > u16::MAX as usize || payload.len() + 6 > self.outlet.mdu() {
            return Err(ChannelError::PayloadTooLarge);
        }

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);

        let envelope = Envelope { msg_type, sequence, payload };
        let raw = envelope.pack();
        self.outlet.send(&raw)?;
        self.pending.insert(sequence, envelope.clone());
        self.states.insert(sequence, MessageState::Sent);
        Ok(sequence)
    }

    pub fn send_typed<M: TypedMessage>(&mut self, message: &M) -> Result<u16, ChannelError> {
        self.send(M::MSG_TYPE, message.encode())
    }

    pub fn resend(&mut self, sequence: u16) -> Result<(), ChannelError> {
        if let Some(envelope) = self.pending.get(&sequence) {
            let raw = envelope.pack();
            self.outlet.resend(&raw)?;
            self.states.insert(sequence, MessageState::Sent);
            return Ok(());
        }
        Err(ChannelError::InvalidFrame)
    }

    pub fn receive(&mut self, raw: &[u8]) -> Result<bool, ChannelError> {
        let envelope = Envelope::unpack(raw)?;
        let distance = envelope.sequence.wrapping_sub(self.next_rx_sequence);
        if distance >= 0x8000 || distance >= 48 {
            return Ok(false);
        }
        if self.rx_ring.insert(envelope.sequence, envelope).is_some() {
            return Ok(false);
        }

        let mut dispatched = false;
        let mut accepted = true;
        while let Some(envelope) = self.rx_ring.remove(&self.next_rx_sequence) {
            dispatched = true;
            self.next_rx_sequence = self.next_rx_sequence.wrapping_add(1);
            let Some(handlers) = self.handlers.get_mut(&envelope.msg_type) else {
                return Err(ChannelError::NoHandler);
            };
            let mut handled = false;
            for registered in handlers {
                match catch_unwind(AssertUnwindSafe(|| (registered.handler)(envelope.clone()))) {
                    Ok(true) => {
                        handled = true;
                        break;
                    }
                    Ok(false) => {}
                    Err(_) => {}
                }
            }
            accepted &= handled;
        }
        Ok(dispatched && accepted)
    }

    pub fn mark_delivered(&mut self, sequence: u16) {
        self.states.insert(sequence, MessageState::Delivered);
        self.pending.remove(&sequence);
    }

    pub fn mark_failed(&mut self, sequence: u16) {
        self.states.insert(sequence, MessageState::Failed);
        self.pending.remove(&sequence);
    }

    pub fn state(&self, sequence: u16) -> MessageState {
        self.states.get(&sequence).copied().unwrap_or(MessageState::New)
    }

    pub fn outlet(&self) -> &O {
        &self.outlet
    }

    pub fn outlet_mut(&mut self) -> &mut O {
        &mut self.outlet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::{Arc, Mutex};

    struct MockOutlet;

    impl ChannelOutlet for MockOutlet {
        fn send(&mut self, _raw: &[u8]) -> Result<(), ChannelError> {
            Ok(())
        }

        fn resend(&mut self, _raw: &[u8]) -> Result<(), ChannelError> {
            Ok(())
        }

        fn mdu(&self) -> usize {
            512
        }

        fn rtt(&self) -> Duration {
            Duration::from_millis(10)
        }

        fn is_usable(&self) -> bool {
            true
        }
    }

    struct LargeMduOutlet;

    impl ChannelOutlet for LargeMduOutlet {
        fn send(&mut self, _raw: &[u8]) -> Result<(), ChannelError> {
            Ok(())
        }

        fn resend(&mut self, _raw: &[u8]) -> Result<(), ChannelError> {
            Ok(())
        }

        fn mdu(&self) -> usize {
            262_144
        }

        fn rtt(&self) -> Duration {
            Duration::from_millis(10)
        }

        fn is_usable(&self) -> bool {
            true
        }
    }

    #[test]
    fn rns_1_5_channel_rejects_payload_above_u16_envelope_limit() {
        let mut channel = Channel::new(LargeMduOutlet);
        assert!(matches!(
            channel.send(0x1234, vec![0x5a; u16::MAX as usize + 1]),
            Err(ChannelError::PayloadTooLarge)
        ));
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ReservedTypedMessage;

    impl TypedMessage for ReservedTypedMessage {
        const MSG_TYPE: u16 = SystemMessageTypes::StreamData as u16;

        fn encode(&self) -> Vec<u8> {
            Vec::new()
        }

        fn decode(_payload: &[u8]) -> Result<Self, ChannelError> {
            Ok(Self)
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SystemTypedMessage;

    impl TypedMessage for SystemTypedMessage {
        const MSG_TYPE: u16 = SystemMessageTypes::StreamData as u16;

        fn is_system_type() -> bool {
            true
        }

        fn encode(&self) -> Vec<u8> {
            Vec::new()
        }

        fn decode(_payload: &[u8]) -> Result<Self, ChannelError> {
            Ok(Self)
        }
    }

    #[test]
    fn typed_messages_reject_reserved_msg_types_by_default() {
        assert!(matches!(
            validate_typed_message_type::<ReservedTypedMessage>(),
            Err(ChannelError::InvalidMessageType)
        ));
    }

    #[test]
    fn system_typed_messages_can_use_reserved_msg_types() {
        let mut channel = Channel::new(MockOutlet);
        let _handler_id = channel
            .register_typed_handler::<SystemTypedMessage, _>(|_message| true)
            .expect("system message types should register");
    }

    #[test]
    fn receive_buffers_out_of_order_messages_until_contiguous() {
        let mut channel = Channel::new(MockOutlet);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        channel.register_handler(0x1234, move |envelope| {
            seen_clone.lock().expect("lock").push((envelope.sequence, envelope.payload));
            true
        });

        let first = Envelope { msg_type: 0x1234, sequence: 0, payload: b"first".to_vec() };
        let second = Envelope { msg_type: 0x1234, sequence: 1, payload: b"second".to_vec() };

        assert!(!channel.receive(&second.pack()).expect("buffer future sequence"));
        assert!(seen.lock().expect("lock").is_empty());

        assert!(channel.receive(&first.pack()).expect("release contiguous"));
        let seen = seen.lock().expect("lock");
        assert_eq!(seen.as_slice(), &[(0, b"first".to_vec()), (1, b"second".to_vec())]);
    }

    #[test]
    fn receive_rejects_duplicate_old_and_outside_window_messages() {
        let mut channel = Channel::new(MockOutlet);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        channel.register_handler(0x1234, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope.sequence);
            true
        });

        let first = Envelope { msg_type: 0x1234, sequence: 0, payload: b"first".to_vec() };
        let duplicate_first = first.clone();
        let far_future = Envelope { msg_type: 0x1234, sequence: 49, payload: b"future".to_vec() };
        let old = Envelope { msg_type: 0x1234, sequence: u16::MAX, payload: b"old".to_vec() };

        assert!(channel.receive(&first.pack()).expect("first"));
        assert!(!channel.receive(&duplicate_first.pack()).expect("duplicate"));
        assert!(!channel.receive(&far_future.pack()).expect("outside window"));
        assert!(!channel.receive(&old.pack()).expect("old sequence"));

        assert_eq!(seen.lock().expect("lock").as_slice(), &[0]);
    }

    #[test]
    fn receive_contains_handler_panics_and_continues_to_later_handlers() {
        let mut channel = Channel::new(MockOutlet);
        let seen = Arc::new(Mutex::new(Vec::new()));
        channel.register_handler(0x1234, |_| -> bool { panic!("boom") });
        let seen_clone = seen.clone();
        channel.register_handler(0x1234, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope.sequence);
            true
        });

        let first = Envelope { msg_type: 0x1234, sequence: 0, payload: b"first".to_vec() };
        let result = catch_unwind(AssertUnwindSafe(|| channel.receive(&first.pack())));

        assert!(result.is_ok(), "handler panic should not unwind channel receive");
        assert!(result.unwrap().expect("receive"));
        assert_eq!(seen.lock().expect("lock").as_slice(), &[0]);
    }
}
