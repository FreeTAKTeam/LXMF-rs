//! Receipt reservations protect an admitted owner until its accepted outcomes commit.
use super::ReceiptEvent;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{error::TrySendError, OwnedPermit, Sender};
use tokio::sync::watch;
mod packets;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustodyCommit {
    Stored,
    SupersededByTerminal,
}

pub struct CustodyCompletion(watch::Receiver<Option<CustodyCommit>>);
impl CustodyCompletion {
    pub fn current(&self) -> Option<CustodyCommit> {
        *self.0.borrow()
    }
    pub async fn wait(&mut self) -> std::io::Result<CustodyCommit> {
        loop {
            if let Some(outcome) = self.current() {
                return Ok(outcome);
            }
            self.0
                .changed()
                .await
                .map_err(|_| std::io::Error::other("custody persistence owner unavailable"))?;
        }
    }
}

enum TerminalSlot {
    Reserved(OwnedPermit<ReceiptEvent>),
    Pending,
}
enum CustodySlot {
    Reserved(OwnedPermit<ReceiptEvent>),
    Pending(watch::Sender<Option<CustodyCommit>>),
}
#[derive(Default)]
struct Reservations {
    terminal: Option<TerminalSlot>,
    custody: Option<CustodySlot>,
}
impl Reservations {
    fn release_unused(&mut self) {
        if matches!(self.terminal, Some(TerminalSlot::Reserved(_))) {
            self.terminal = None;
        }
        if matches!(self.custody, Some(CustodySlot::Reserved(_))) {
            self.custody = None;
        }
    }
    fn is_empty(&self) -> bool {
        self.terminal.is_none() && self.custody.is_none()
    }
}
#[derive(Clone)]
pub struct ReceiptPublisher {
    sender: Sender<ReceiptEvent>,
    reservations: Arc<Mutex<HashMap<String, Reservations>>>,
    completion: tokio::sync::watch::Sender<()>,
    stopping: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    packets: Arc<Mutex<HashMap<[u8; 32], packets::PacketCorrelation>>>,
}
impl From<Sender<ReceiptEvent>> for ReceiptPublisher {
    fn from(sender: Sender<ReceiptEvent>) -> Self {
        Self {
            sender,
            reservations: Arc::new(Mutex::new(HashMap::new())),
            completion: tokio::sync::watch::channel(()).0,
            stopping: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(AtomicBool::new(false)),
            packets: Default::default(),
        }
    }
}
impl ReceiptPublisher {
    pub fn reserve_terminal(&self, message_id: &str) -> std::io::Result<()> {
        self.reserve_completion(message_id, false)
    }
    pub fn reserve_completion(&self, message_id: &str, custody: bool) -> std::io::Result<()> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "terminal receipt admission is stopping",
            ));
        }
        let mut reservations =
            self.reservations.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        if self.stopping.load(Ordering::Acquire) || self.failed.load(Ordering::Acquire) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "receipt admission is stopping",
            ));
        }
        let existing = reservations.get(message_id);
        // Acquire everything before changing ownership; a failed second reservation
        // drops the first permit and leaves existing pending work untouched.
        let terminal = if existing.is_none_or(|s| s.terminal.is_none()) {
            Some(self.reserve_slot()?)
        } else {
            None
        };
        let custody = if custody && existing.is_none_or(|s| s.custody.is_none()) {
            Some(self.reserve_slot()?)
        } else {
            None
        };
        let slots = reservations.entry(message_id.into()).or_default();
        if let Some(permit) = terminal {
            slots.terminal = Some(TerminalSlot::Reserved(permit));
        }
        if let Some(permit) = custody {
            slots.custody = Some(CustodySlot::Reserved(permit));
        }
        Ok(())
    }
    fn reserve_slot(&self) -> std::io::Result<OwnedPermit<ReceiptEvent>> {
        self.sender.clone().try_reserve_owned().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                format!("terminal receipt capacity unavailable: {e}"),
            )
        })
    }
    pub fn retire_committed(
        &self,
        mut terminal: impl FnMut(&str) -> std::io::Result<bool>,
    ) -> std::io::Result<()> {
        let candidates: Vec<String> = self
            .reservations
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .iter()
            .filter(|(_, slots)| {
                matches!(slots.terminal, Some(TerminalSlot::Reserved(_)))
                    || matches!(slots.custody, Some(CustodySlot::Reserved(_)))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in candidates {
            if terminal(&id)? {
                self.release_unsent(&id);
            }
        }
        Ok(())
    }
    pub fn try_send(&self, mut event: ReceiptEvent) -> std::io::Result<()> {
        if event.custody().is_some() {
            return self.publish_custody(event).map(|_| ());
        }
        if self.failed.load(Ordering::Acquire) || self.sender.is_closed() {
            return Err(std::io::Error::other("receipt persistence owner failed"));
        }
        if is_terminal(&event.status) {
            let mut reservations = match self.reservations.lock() {
                Ok(value) => value,
                Err(error) => {
                    log::error!("terminal receipt reservation poisoned: {error}");
                    return Err(std::io::Error::other("terminal receipt reservation poisoned"));
                }
            };
            if self.failed.load(Ordering::Acquire) || self.sender.is_closed() {
                return Err(std::io::Error::other("receipt persistence owner failed"));
            }
            if let Some(slot) =
                reservations.get_mut(&event.message_id).and_then(|s| s.terminal.take())
            {
                let id = event.message_id.clone();
                if let TerminalSlot::Reserved(permit) = slot {
                    event.bound_completion_fields();
                    permit.send(event);
                }
                reservations
                    .get_mut(&id)
                    .expect("terminal reservation exists under lock")
                    .terminal = Some(TerminalSlot::Pending);
                // A duplicate callback shares the persistence owner until commit.
                return Ok(());
            }
            // Unreserved legacy terminal receipts also have a persistence owner
            // once enqueue succeeds. Hold the map lock until registration so a
            // fast consumer cannot acknowledge before the pending entry exists.
            let id = event.message_id.clone();
            self.send_unreserved(event)?;
            reservations.entry(id).or_default().terminal = Some(TerminalSlot::Pending);
            return Ok(());
        }
        self.send_unreserved(event)
    }
    pub fn publish_custody(&self, mut event: ReceiptEvent) -> std::io::Result<CustodyCompletion> {
        let representation = event.custody().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "custody publication requires a typed acknowledgement",
            )
        })?;
        if self.failed.load(Ordering::Acquire) || self.sender.is_closed() {
            return Err(std::io::Error::other("receipt persistence owner failed"));
        }
        let mut reservations =
            self.reservations.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        if self.failed.load(Ordering::Acquire) || self.sender.is_closed() {
            return Err(std::io::Error::other("receipt persistence owner failed"));
        }
        if let Some(CustodySlot::Pending(completion)) =
            reservations.get(&event.message_id).and_then(|s| s.custody.as_ref())
        {
            return Ok(CustodyCompletion(completion.subscribe()));
        }
        if self.stopping.load(Ordering::Acquire)
            && reservations.get(&event.message_id).is_none_or(|s| s.custody.is_none())
        {
            return Err(std::io::Error::other("custody admission is stopping"));
        }
        let id = event.message_id.clone();
        event.status = representation.status().into();
        event.bound_completion_fields();
        let (completion, observer) = watch::channel(None);
        match reservations.get_mut(&id).and_then(|s| s.custody.take()) {
            Some(CustodySlot::Reserved(permit)) => {
                permit.send(event);
            }
            Some(CustodySlot::Pending(_)) => {
                unreachable!("pending custody was handled under the same lock")
            }
            None => self.send_unreserved(event)?,
        }
        reservations.entry(id).or_default().custody = Some(CustodySlot::Pending(completion));
        Ok(CustodyCompletion(observer))
    }
    fn send_unreserved(&self, event: ReceiptEvent) -> std::io::Result<()> {
        self.sender.try_send(event).map_err(|error| match error {
            TrySendError::Full(_) => {
                std::io::Error::new(std::io::ErrorKind::WouldBlock, "receipt queue full")
            }
            TrySendError::Closed(_) => {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "receipt owner closed")
            }
        })
    }
    /// True only after a terminal receipt was accepted for persistence.
    /// An unused reservation is not pending, and queue acceptance is not commit.
    pub fn terminal_pending(&self, message_id: &str) -> std::io::Result<bool> {
        let reservations =
            self.reservations.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(reservations
            .get(message_id)
            .is_some_and(|s| matches!(s.terminal, Some(TerminalSlot::Pending))))
    }
    pub fn terminal_completion_events(&self) -> tokio::sync::watch::Receiver<()> {
        self.completion.subscribe()
    }
    pub fn complete_terminal(&self, message_id: &str) {
        match self.reservations.lock() {
            Ok(mut slots) => {
                if let Some(reservations) = slots.get_mut(message_id) {
                    reservations.terminal = None;
                    reservations.release_unused();
                    if reservations.is_empty() {
                        slots.remove(message_id);
                    }
                    self.completion.send_replace(());
                }
            }
            Err(error) => log::error!("terminal receipt completion poisoned: {error}"),
        }
        self.prune_link_packets(message_id, false);
    }
    /// Called by the sole receipt worker after persistence. This also retires
    /// unused dispatch-owner reservations; accepted terminal work remains fenced.
    pub fn complete_custody(&self, message_id: &str, outcome: CustodyCommit) {
        match self.reservations.lock() {
            Ok(mut slots) => {
                if let Some(reservations) = slots.get_mut(message_id) {
                    if let Some(CustodySlot::Pending(completion)) = reservations.custody.take() {
                        completion.send_replace(Some(outcome));
                        reservations.release_unused();
                    }
                    if reservations.is_empty() {
                        slots.remove(message_id);
                    }
                }
            }
            Err(error) => log::error!("custody receipt completion poisoned: {error}"),
        }
        self.prune_link_packets(message_id, outcome == CustodyCommit::Stored);
    }
    pub fn release_unsent(&self, message_id: &str) {
        match self.reservations.lock() {
            Ok(mut slots) => {
                if let Some(reservations) = slots.get_mut(message_id) {
                    reservations.release_unused();
                    if reservations.is_empty() {
                        slots.remove(message_id);
                    }
                }
            }
            Err(error) => log::error!("terminal receipt release poisoned: {error}"),
        }
    }
    pub fn fail_owner(&self) {
        self.failed.store(true, Ordering::Release);
        self.stop_admission();
        match self.reservations.lock() {
            Ok(mut slots) => {
                for reservations in slots.values_mut() {
                    reservations.custody = None;
                }
            }
            Err(error) => log::error!("custody receipt failure notification poisoned: {error}"),
        }
    }
    pub fn stop_admission(&self) {
        self.stopping.store(true, Ordering::Release);
    }
    /// Call after network producers stop. Unobserved outcomes retain their SQLite dispatch intents.
    pub fn release_unobserved(&self) {
        match self.reservations.lock() {
            Ok(mut slots) => slots.retain(|_, reservations| {
                reservations.release_unused();
                !reservations.is_empty()
            }),
            Err(error) => log::error!("terminal receipt shutdown poisoned: {error}"),
        }
    }
}
pub fn is_terminal(status: &str) -> bool {
    let value = status.trim().to_ascii_lowercase();
    matches!(value.as_str(), "delivered" | "cancelled" | "expired" | "rejected")
        || value.starts_with("failed")
}
#[cfg(test)]
mod tests;
