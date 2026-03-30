use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeEvent {
    IdentityReady {
        destination_hash: [u8; 16],
    },
    AnnounceQueued {
        destination_hash: [u8; 16],
    },
    AnnounceSkipped {
        destination_hash: [u8; 16],
    },
    AnnounceReceived {
        destination_hash: [u8; 16],
        app_data: Vec<u8>,
    },
    MessageQueued {
        destination_hash: [u8; 16],
        message_id: [u8; 32],
    },
    MessageReceived {
        source_hash: [u8; 16],
        message_id: [u8; 32],
        verified: bool,
        content: Vec<u8>,
    },
    TelemetryRecorded {
        inserted: usize,
    },
    TelemetryDropped {
        dropped: usize,
    },
}
