use crate::*;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard};

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    mutex.lock().map_err(|_| BluetoothError::new(ErrorKind::Platform, "state lock"))
}

/// Process-local admission only. No global controller reservation is implied.
pub struct SessionRegistry {
    capacity: usize,
    state: Mutex<RegistryState>,
}
#[derive(Default)]
struct RegistryState {
    generation: u64,
    sessions: BTreeMap<SessionId, DeviceId>,
}
impl SessionRegistry {
    pub fn new(capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(BluetoothError::new(ErrorKind::InvalidInput, "session capacity"));
        }
        Ok(Self { capacity, state: Mutex::new(RegistryState::default()) })
    }
    /// Reserve before starting native open. Failed/cancelled opens must release this handle.
    pub fn reserve(&self, owner: OwnerId, device: DeviceId) -> Result<SessionId> {
        if owner.0.is_empty() || device.0.is_empty() {
            return Err(BluetoothError::new(ErrorKind::InvalidInput, "session identity"));
        }
        let mut state = lock(&self.state)?;
        if state.sessions.len() >= self.capacity || state.sessions.values().any(|d| *d == device) {
            return Err(BluetoothError::new(ErrorKind::Busy, "reserve session"));
        }
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| BluetoothError::new(ErrorKind::Unavailable, "session generation"))?;
        let id = SessionId { owner, generation: state.generation };
        state.sessions.insert(id.clone(), device);
        Ok(id)
    }
    pub fn device(&self, id: &SessionId) -> Result<DeviceId> {
        lock(&self.state)?
            .sessions
            .get(id)
            .cloned()
            .ok_or_else(|| BluetoothError::new(ErrorKind::StaleSession, "session lookup"))
    }
    /// Idempotent release cannot affect another generation/owner.
    pub fn release(&self, id: &SessionId) -> Result<bool> {
        Ok(lock(&self.state)?.sessions.remove(id).is_some())
    }
}

struct QueueState {
    sequence: u64,
    events: VecDeque<BluetoothEvent>,
    terminal: Option<BluetoothError>,
}
/// One owner per queue. Overflow terminates the stream rather than silently losing data.
pub struct EventQueue {
    owner: OwnerId,
    limits: Limits,
    state: Mutex<QueueState>,
    changed: Condvar,
}
impl EventQueue {
    pub fn new(owner: OwnerId, limits: Limits) -> Result<Self> {
        Ok(Self {
            owner,
            limits: limits.validate()?,
            changed: Condvar::new(),
            state: Mutex::new(QueueState { sequence: 0, events: VecDeque::new(), terminal: None }),
        })
    }
    pub fn push(
        &self,
        session: SessionId,
        operation_id: Option<String>,
        kind: EventKind,
    ) -> Result<()> {
        if session.owner != self.owner {
            return Err(BluetoothError::new(ErrorKind::StaleSession, "event owner"));
        }
        let mut state = lock(&self.state)?;
        if let Some(error) = &state.terminal {
            return Err(error.clone());
        }
        let error = if matches!(&kind, EventKind::Notification { payload, .. }
            if payload.len() > self.limits.max_payload_bytes)
        {
            Some(BluetoothError::new(ErrorKind::PayloadTooLarge, "event payload"))
        } else if state.events.len() >= self.limits.max_events {
            Some(BluetoothError::new(ErrorKind::QueueFull, "event queue"))
        } else {
            None
        };
        if let Some(error) = error {
            state.terminal = Some(error.clone());
            state.events.clear();
            self.changed.notify_all();
            return Err(error);
        }
        state.sequence = state
            .sequence
            .checked_add(1)
            .ok_or_else(|| BluetoothError::new(ErrorKind::Unavailable, "event sequence"))?;
        let sequence = state.sequence;
        state.events.push_back(BluetoothEvent { sequence, session, operation_id, kind });
        self.changed.notify_one();
        Ok(())
    }
    pub fn poll(&self, deadline: Deadline) -> Result<Option<BluetoothEvent>> {
        let mut state = lock(&self.state)?;
        loop {
            if let Some(error) = &state.terminal {
                return Err(error.clone());
            }
            if let Some(event) = state.events.pop_front() {
                return Ok(Some(event));
            }
            let remaining = match deadline.remaining() {
                Ok(remaining) => remaining,
                Err(_) => return Ok(None),
            };
            let (next, _) = self
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| BluetoothError::new(ErrorKind::Platform, "event wait"))?;
            state = next;
        }
    }
    pub fn close(&self) -> Result<()> {
        let mut state = lock(&self.state)?;
        state
            .terminal
            .get_or_insert_with(|| BluetoothError::new(ErrorKind::Closed, "event stream"));
        state.events.clear();
        self.changed.notify_all();
        Ok(())
    }
}
