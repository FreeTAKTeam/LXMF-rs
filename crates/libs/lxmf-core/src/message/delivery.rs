use crate::constants::{
    ENCRYPTED_PACKET_MAX_CONTENT, LINK_PACKET_MAX_CONTENT, PAPER_MDU, PLAIN_PACKET_MAX_CONTENT,
};
use crate::error::LxmfError;
use crate::message::{MessageMethod, TransportMethod};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryDecision {
    pub method: TransportMethod,
    pub representation: MessageMethod,
}

pub fn decide_delivery(
    desired_method: TransportMethod,
    destination_is_plain: bool,
    content_size: usize,
) -> Result<DeliveryDecision, LxmfError> {
    let mut method = desired_method;

    if matches!(method, TransportMethod::Opportunistic) {
        let limit = if destination_is_plain {
            PLAIN_PACKET_MAX_CONTENT
        } else {
            ENCRYPTED_PACKET_MAX_CONTENT
        };

        if content_size > limit {
            method = TransportMethod::Direct;
        } else {
            return Ok(DeliveryDecision { method, representation: MessageMethod::Packet });
        }
    }

    match method {
        TransportMethod::Direct => {
            let representation = if content_size <= LINK_PACKET_MAX_CONTENT {
                MessageMethod::Packet
            } else {
                MessageMethod::Resource
            };
            Ok(DeliveryDecision { method, representation })
        }
        TransportMethod::Propagated => {
            let representation = if content_size <= LINK_PACKET_MAX_CONTENT {
                MessageMethod::Packet
            } else {
                MessageMethod::Resource
            };
            Ok(DeliveryDecision { method, representation })
        }
        TransportMethod::Paper => {
            if content_size <= PAPER_MDU {
                Ok(DeliveryDecision { method, representation: MessageMethod::Paper })
            } else {
                Err(LxmfError::Encode("paper delivery content exceeds paper MDU".into()))
            }
        }
        TransportMethod::Opportunistic => {
            Err(LxmfError::Encode("opportunistic delivery could not be resolved".into()))
        }
    }
}
