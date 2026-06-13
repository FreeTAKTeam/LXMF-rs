use super::init::LXMF_PEER_SYNC_BACKOFF_STEP_SECS;

use super::*;

pub(super) const PEER_SYNC_STATE_IDLE: u32 = 0x00;

pub(super) const PEER_SYNC_STATE_FAILED: u32 = 0xfe;
