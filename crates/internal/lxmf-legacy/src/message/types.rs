#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageState {
    Generating = 0x00,
    Outbound = 0x01,
    Sending = 0x02,
    Sent = 0x04,
    Delivered = 0x08,
    Rejected = 0xFD,
    Cancelled = 0xFE,
    Failed = 0xFF,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageMethod {
    Unknown = 0x00,
    Packet = 0x01,
    Resource = 0x02,
    Paper = 0x05,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMethod {
    Opportunistic = 0x01,
    Direct = 0x02,
    Propagated = 0x03,
    Paper = 0x05,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnverifiedReason {
    SourceUnknown = 0x01,
    SignatureInvalid = 0x02,
}

impl MessageState {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for MessageState {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(MessageState::Generating),
            0x01 => Ok(MessageState::Outbound),
            0x02 => Ok(MessageState::Sending),
            0x04 => Ok(MessageState::Sent),
            0x08 => Ok(MessageState::Delivered),
            0xFD => Ok(MessageState::Rejected),
            0xFE => Ok(MessageState::Cancelled),
            0xFF => Ok(MessageState::Failed),
            _ => Err(()),
        }
    }
}

impl UnverifiedReason {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for UnverifiedReason {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(UnverifiedReason::SourceUnknown),
            0x02 => Ok(UnverifiedReason::SignatureInvalid),
            _ => Err(()),
        }
    }
}

impl MessageMethod {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for MessageMethod {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(MessageMethod::Unknown),
            0x01 => Ok(MessageMethod::Packet),
            0x02 => Ok(MessageMethod::Resource),
            0x05 => Ok(MessageMethod::Paper),
            _ => Err(()),
        }
    }
}

impl TransportMethod {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for TransportMethod {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(TransportMethod::Opportunistic),
            0x02 => Ok(TransportMethod::Direct),
            0x03 => Ok(TransportMethod::Propagated),
            0x05 => Ok(TransportMethod::Paper),
            _ => Err(()),
        }
    }
}
