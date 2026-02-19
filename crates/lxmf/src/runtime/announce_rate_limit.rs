use super::{now_epoch_secs, AnnounceTarget};
use reticulum::transport::Transport;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub(super) fn trigger_rate_limited_announce(
    transport: &Arc<Transport>,
    announce_targets: &[AnnounceTarget],
    last_announce_epoch_secs: &Arc<AtomicU64>,
    min_interval_secs: u64,
) {
    if !try_acquire_announce_window(last_announce_epoch_secs, min_interval_secs) {
        return;
    }
    let announce_transport = transport.clone();
    let announce_targets = announce_targets.to_vec();
    tokio::spawn(async move {
        for target in announce_targets {
            announce_transport.send_announce(&target.destination, target.app_data.as_deref()).await;
        }
    });
}

fn try_acquire_announce_window(
    last_announce_epoch_secs: &Arc<AtomicU64>,
    min_interval_secs: u64,
) -> bool {
    let now = now_epoch_secs();
    loop {
        let previous = last_announce_epoch_secs.load(Ordering::Relaxed);
        if previous != 0 && now.saturating_sub(previous) < min_interval_secs {
            return false;
        }
        if last_announce_epoch_secs
            .compare_exchange(previous, now, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return true;
        }
    }
}
