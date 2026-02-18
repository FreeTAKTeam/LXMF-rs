use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use reticulum::rpc::{AnnounceBridge, RpcDaemon};
use reticulum::storage::messages::MessagesStore;
use tokio::task::LocalSet;
use tokio::time::{advance, Duration};

struct CounterAnnounceBridge {
    calls: AtomicUsize,
}

impl CounterAnnounceBridge {
    fn new() -> Self {
        Self { calls: AtomicUsize::new(0) }
    }
}

impl AnnounceBridge for CounterAnnounceBridge {
    fn announce_now(&self) -> Result<(), std::io::Error> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn announce_scheduler_emits_event_after_interval() {
    let daemon = Rc::new(RpcDaemon::test_instance());
    let local = LocalSet::new();

    local
        .run_until(async move {
            let _handle = daemon.clone().start_announce_scheduler(1);

            tokio::task::yield_now().await;
            advance(Duration::from_secs(1)).await;
            tokio::task::yield_now().await;

            let event = daemon.take_event().expect("announce event");
            assert_eq!(event.event_type, "announce_sent");
        })
        .await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn announce_scheduler_calls_announce_bridge_immediately() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let bridge = Arc::new(CounterAnnounceBridge::new());
    let daemon = Rc::new(RpcDaemon::with_store_and_bridges(
        store,
        "test-identity".into(),
        None,
        Some(bridge.clone()),
    ));
    let local = LocalSet::new();

    local
        .run_until(async move {
            let _handle = daemon.clone().start_announce_scheduler(30);

            tokio::task::yield_now().await;
            assert_eq!(bridge.calls.load(Ordering::Relaxed), 1);

            let event = daemon.take_event().expect("announce event");
            assert_eq!(event.event_type, "announce_sent");

            advance(Duration::from_secs(30)).await;
            tokio::task::yield_now().await;
            assert_eq!(bridge.calls.load(Ordering::Relaxed), 2);
        })
        .await;
}
