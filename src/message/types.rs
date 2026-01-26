#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageState {
    Generating,
    Outbound,
    Sending,
    Sent,
    Delivered,
    Rejected,
    Cancelled,
    Failed,
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
    SourceUnknown,
    SignatureInvalid,
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
