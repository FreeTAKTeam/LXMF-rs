use std::collections::HashMap;
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
    next_handler_id: u64,
    handlers: HashMap<u16, Vec<RegisteredHandler>>,
    pending: HashMap<u16, Envelope>,
    states: HashMap<u16, MessageState>,
}

impl<O: ChannelOutlet> Channel<O> {
    pub fn new(outlet: O) -> Self {
        Self {
            outlet,
            next_sequence: 0,
            next_handler_id: 0,
            handlers: HashMap::new(),
            pending: HashMap::new(),
            states: HashMap::new(),
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
        if payload.len() + 6 > self.outlet.mdu() {
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
        let Some(handlers) = self.handlers.get_mut(&envelope.msg_type) else {
            return Err(ChannelError::NoHandler);
        };
        for registered in handlers {
            if (registered.handler)(envelope.clone()) {
                return Ok(true);
            }
        }
        Ok(false)
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
}
