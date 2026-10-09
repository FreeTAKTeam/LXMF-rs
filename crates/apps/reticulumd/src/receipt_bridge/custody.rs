//! Reserved terminal receipt capacity belongs to an admitted durable operation.
use super::ReceiptEvent;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{error::TrySendError, OwnedPermit, Sender};
#[derive(Clone)]
pub struct ReceiptPublisher {
    sender: Sender<ReceiptEvent>,
    terminal: Arc<Mutex<HashMap<String, Option<OwnedPermit<ReceiptEvent>>>>>,
    stopping: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
}
impl From<Sender<ReceiptEvent>> for ReceiptPublisher {
    fn from(sender: Sender<ReceiptEvent>) -> Self {
        Self {
            sender,
            terminal: Arc::new(Mutex::new(HashMap::new())),
            stopping: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl ReceiptPublisher {
    pub fn reserve_terminal(&self, message_id: &str) -> std::io::Result<()> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "terminal receipt admission is stopping",
            ));
        }
        let mut terminal =
            self.terminal.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        if terminal.contains_key(message_id) {
            return Ok(());
        }
        let permit = self.sender.clone().try_reserve_owned().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                format!("terminal receipt capacity unavailable: {e}"),
            )
        })?;
        terminal.insert(message_id.into(), Some(permit));
        Ok(())
    }
    pub fn retire_committed(
        &self,
        mut terminal: impl FnMut(&str) -> std::io::Result<bool>,
    ) -> std::io::Result<()> {
        let candidates: Vec<String> = self
            .terminal
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .iter()
            .filter(|(_, permit)| permit.is_some())
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
        if self.failed.load(Ordering::Acquire) || self.sender.is_closed() {
            return Err(std::io::Error::other("receipt persistence owner failed"));
        }
        if is_terminal(&event.status) {
            let mut terminal = match self.terminal.lock() {
                Ok(value) => value,
                Err(error) => {
                    log::error!("terminal receipt reservation poisoned: {error}");
                    return Err(std::io::Error::other("terminal receipt reservation poisoned"));
                }
            };
            if let Some(slot) = terminal.get_mut(&event.message_id) {
                if let Some(permit) = slot.take() {
                    event.status = event.status.chars().take(512).collect();
                    for field in [
                        &mut event.packet_hash,
                        &mut event.resource_hash,
                        &mut event.peer,
                        &mut event.method,
                        &mut event.delivery_kind,
                        &mut event.link_id,
                        &mut event.stage,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let value = field;
                        value.truncate(
                            value.char_indices().nth(128).map_or(value.len(), |(index, _)| index),
                        );
                    }
                    permit.send(event);
                }
                // A duplicate callback shares the persistence owner until commit.
                return Ok(());
            }
        }
        self.sender.try_send(event).map_err(|error| match error {
            TrySendError::Full(_) => {
                std::io::Error::new(std::io::ErrorKind::WouldBlock, "receipt queue full")
            }
            TrySendError::Closed(_) => {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "receipt owner closed")
            }
        })
    }
    pub fn complete_terminal(&self, message_id: &str) {
        match self.terminal.lock() {
            Ok(mut slots) => {
                slots.remove(message_id);
            }
            Err(error) => log::error!("terminal receipt completion poisoned: {error}"),
        }
    }
    pub fn release_unsent(&self, message_id: &str) {
        match self.terminal.lock() {
            Ok(mut slots) => {
                if slots.get(message_id).is_some_and(Option::is_some) {
                    slots.remove(message_id);
                }
            }
            Err(error) => log::error!("terminal receipt release poisoned: {error}"),
        }
    }
    pub fn fail_owner(&self) {
        self.failed.store(true, Ordering::Release);
        self.stop_admission();
    }
    pub fn stop_admission(&self) {
        self.stopping.store(true, Ordering::Release);
    }
    /// Call after network producers stop. Unobserved outcomes retain their SQLite dispatch intents.
    pub fn release_unobserved(&self) {
        match self.terminal.lock() {
            Ok(mut slots) => slots.retain(|_, permit| permit.is_none()),
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
mod tests {
    use super::*;
    #[tokio::test]
    async fn reserved_terminal_survives_full_progress_queue_and_duplicate_callback() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
        let publisher = ReceiptPublisher::from(sender);
        publisher.reserve_terminal("durable").unwrap();
        publisher.try_send(ReceiptEvent::new("legacy", "sending")).unwrap();
        assert!(publisher.try_send(ReceiptEvent::new("legacy", "sending")).is_err());
        publisher.try_send(ReceiptEvent::new("durable", "delivered")).unwrap();
        publisher.try_send(ReceiptEvent::new("durable", "delivered")).unwrap();
        assert_eq!(receiver.recv().await.unwrap().message_id, "legacy");
        assert_eq!(receiver.recv().await.unwrap().message_id, "durable");
        assert!(receiver.try_recv().is_err());
        publisher.complete_terminal("durable");
        publisher.stop_admission();
        assert!(publisher.reserve_terminal("next").is_err());
    }
}
