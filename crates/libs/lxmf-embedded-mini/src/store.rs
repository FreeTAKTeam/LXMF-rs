use crate::MiniResult;

pub trait MiniStore {
    fn load_replay_floor(&self, identity: &[u8; 16]) -> MiniResult<u64>;
    fn save_replay_floor(&mut self, identity: &[u8; 16], floor: u64) -> MiniResult<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NullStore;

impl MiniStore for NullStore {
    fn load_replay_floor(&self, _identity: &[u8; 16]) -> MiniResult<u64> {
        Ok(0)
    }

    fn save_replay_floor(&mut self, _identity: &[u8; 16], _floor: u64) -> MiniResult<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RamReplayStore {
    identity: [u8; 16],
    replay_floor: u64,
    initialized: bool,
}

impl RamReplayStore {
    pub const fn new() -> Self {
        Self { identity: [0; 16], replay_floor: 0, initialized: false }
    }
}

impl Default for RamReplayStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MiniStore for RamReplayStore {
    fn load_replay_floor(&self, identity: &[u8; 16]) -> MiniResult<u64> {
        if self.initialized && &self.identity == identity {
            Ok(self.replay_floor)
        } else {
            Ok(0)
        }
    }

    fn save_replay_floor(&mut self, identity: &[u8; 16], floor: u64) -> MiniResult<()> {
        self.identity = *identity;
        self.replay_floor = floor;
        self.initialized = true;
        Ok(())
    }
}
