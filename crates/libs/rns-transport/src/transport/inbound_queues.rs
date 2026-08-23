use std::collections::VecDeque;
use std::sync::Mutex;

use tokio::sync::Notify;

pub const DEFAULT_DATA_QUEUE_LENGTH: usize = 4096;
pub const DEFAULT_ANNOUNCE_QUEUE_LENGTH: usize = 256;
pub const DEFAULT_PATH_REQUEST_QUEUE_LENGTH: usize = 256;
pub const DEFAULT_INGRESS_LIMITED_QUEUE_LENGTH: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum InboundTrafficClass {
    Data = 0,
    Announce = 1,
    PathRequest = 2,
    IngressLimited = 3,
}

impl InboundTrafficClass {
    const ALL: [Self; 4] = [Self::Data, Self::Announce, Self::PathRequest, Self::IngressLimited];

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboundQueueLimits {
    pub data: usize,
    pub announce: usize,
    pub path_request: usize,
    pub ingress_limited: usize,
}

impl Default for InboundQueueLimits {
    fn default() -> Self {
        Self {
            data: DEFAULT_DATA_QUEUE_LENGTH,
            announce: DEFAULT_ANNOUNCE_QUEUE_LENGTH,
            path_request: DEFAULT_PATH_REQUEST_QUEUE_LENGTH,
            ingress_limited: DEFAULT_INGRESS_LIMITED_QUEUE_LENGTH,
        }
    }
}

impl InboundQueueLimits {
    pub(crate) fn as_array(self) -> [usize; 4] {
        [self.data, self.announce, self.path_request, self.ingress_limited]
    }

    pub fn is_valid(self) -> bool {
        self.as_array().into_iter().all(|limit| limit > 0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InboundQueueSnapshot {
    pub total: usize,
    pub limits: [usize; 4],
    pub heights: [usize; 4],
    pub dropped: [u64; 4],
}

impl InboundQueueSnapshot {
    pub fn height(self, class: InboundTrafficClass) -> usize {
        self.heights[class.index()]
    }

    pub fn dropped(self, class: InboundTrafficClass) -> u64 {
        self.dropped[class.index()]
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InboundQueueFull<T> {
    pub class: InboundTrafficClass,
    pub item: T,
}

struct QueueState<T> {
    queues: [VecDeque<T>; 4],
    limits: [usize; 4],
    dropped: [u64; 4],
}

pub struct InboundQueues<T> {
    state: Mutex<QueueState<T>>,
    available: Notify,
}

impl<T> InboundQueues<T> {
    pub fn new(limits: InboundQueueLimits) -> Self {
        assert!(limits.is_valid(), "inbound queue limits must be non-zero");
        Self {
            state: Mutex::new(QueueState {
                queues: std::array::from_fn(|_| VecDeque::new()),
                limits: limits.as_array(),
                dropped: [0; 4],
            }),
            available: Notify::new(),
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, QueueState<T>> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn enqueue(&self, class: InboundTrafficClass, item: T) -> Result<(), InboundQueueFull<T>> {
        let mut state = self.lock_state();
        let index = class.index();
        if state.queues[index].len() >= state.limits[index] {
            state.dropped[index] = state.dropped[index].saturating_add(1);
            return Err(InboundQueueFull { class, item });
        }
        state.queues[index].push_back(item);
        drop(state);
        self.available.notify_one();
        Ok(())
    }

    pub fn try_dequeue(&self) -> Option<T> {
        let mut state = self.lock_state();
        for class in InboundTrafficClass::ALL {
            if let Some(item) = state.queues[class.index()].pop_front() {
                return Some(item);
            }
        }
        None
    }

    pub async fn dequeue(&self) -> T {
        loop {
            let notified = self.available.notified();
            if let Some(item) = self.try_dequeue() {
                return item;
            }
            notified.await;
        }
    }

    pub fn snapshot(&self) -> InboundQueueSnapshot {
        let state = self.lock_state();
        let heights = std::array::from_fn(|index| state.queues[index].len());
        InboundQueueSnapshot {
            total: heights.iter().sum(),
            limits: state.limits,
            heights,
            dropped: state.dropped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    fn tiny_queues() -> InboundQueues<&'static str> {
        InboundQueues::new(InboundQueueLimits {
            data: 2,
            announce: 2,
            path_request: 2,
            ingress_limited: 2,
        })
    }

    #[test]
    fn rns_1_5_inbound_queues_use_the_exact_total_order() {
        let classes = InboundTrafficClass::ALL;
        for (higher_index, higher) in classes.iter().enumerate() {
            for lower in classes.iter().skip(higher_index + 1) {
                let queues = tiny_queues();
                queues.enqueue(*lower, "lower").expect("lower enqueue");
                queues.enqueue(*higher, "higher").expect("higher enqueue");
                assert_eq!(queues.try_dequeue(), Some("higher"));
                assert_eq!(queues.try_dequeue(), Some("lower"));
            }
        }
    }

    #[test]
    fn rns_1_5_inbound_queues_are_fifo_within_each_class() {
        for class in InboundTrafficClass::ALL {
            let queues = tiny_queues();
            queues.enqueue(class, "first").expect("first enqueue");
            queues.enqueue(class, "second").expect("second enqueue");
            assert_eq!(queues.try_dequeue(), Some("first"));
            assert_eq!(queues.try_dequeue(), Some("second"));
        }
    }

    #[test]
    fn rns_1_5_inbound_queues_track_full_drops_and_consistent_snapshots() {
        let queues = tiny_queues();
        queues.enqueue(InboundTrafficClass::Data, "one").expect("enqueue one");
        queues.enqueue(InboundTrafficClass::Data, "two").expect("enqueue two");
        let error = queues
            .enqueue(InboundTrafficClass::Data, "three")
            .expect_err("third item must be dropped");
        assert_eq!(error.item, "three");
        let snapshot = queues.snapshot();
        assert_eq!(snapshot.total, 2);
        assert_eq!(snapshot.height(InboundTrafficClass::Data), 2);
        assert_eq!(snapshot.dropped(InboundTrafficClass::Data), 1);
    }

    #[tokio::test]
    async fn rns_1_5_inbound_queues_wake_a_waiting_drainer() {
        let queues = Arc::new(tiny_queues());
        let waiter = {
            let queues = queues.clone();
            tokio::spawn(async move { queues.dequeue().await })
        };
        tokio::task::yield_now().await;
        queues.enqueue(InboundTrafficClass::Announce, "ready").expect("enqueue");
        assert_eq!(waiter.await.expect("waiter join"), "ready");
    }

    #[tokio::test]
    async fn rns_1_5_inbound_queues_wait_is_cancellation_safe() {
        let queues = Arc::new(tiny_queues());
        let timed_out = tokio::time::timeout(Duration::from_millis(1), queues.dequeue()).await;
        assert!(timed_out.is_err());
        queues.enqueue(InboundTrafficClass::Data, "retained").expect("enqueue");
        assert_eq!(queues.dequeue().await, "retained");
    }

    #[test]
    fn rns_1_5_inbound_queues_intentionally_starve_lower_classes() {
        let queues = tiny_queues();
        queues.enqueue(InboundTrafficClass::IngressLimited, "low").expect("low enqueue");
        for _ in 0..2 {
            queues.enqueue(InboundTrafficClass::Data, "high").expect("high enqueue");
            assert_eq!(queues.try_dequeue(), Some("high"));
        }
        assert_eq!(queues.try_dequeue(), Some("low"));
    }
}
