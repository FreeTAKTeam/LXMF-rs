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
    PayloadTooLarge,
    InvalidFrame,
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

pub struct Channel<O: ChannelOutlet> {
    outlet: O,
    next_sequence: u16,
    handlers: HashMap<u16, Handler>,
    pending: HashMap<u16, Envelope>,
    states: HashMap<u16, MessageState>,
}

impl<O: ChannelOutlet> Channel<O> {
    pub fn new(outlet: O) -> Self {
        Self {
            outlet,
            next_sequence: 0,
            handlers: HashMap::new(),
            pending: HashMap::new(),
            states: HashMap::new(),
        }
    }

    pub fn register_handler<F>(&mut self, msg_type: u16, handler: F)
    where
        F: FnMut(Envelope) -> bool + Send + 'static,
    {
        self.handlers.insert(msg_type, Box::new(handler));
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
        let Some(handler) = self.handlers.get_mut(&envelope.msg_type) else {
            return Err(ChannelError::NoHandler);
        };
        Ok(handler(envelope))
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
