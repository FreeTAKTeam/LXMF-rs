use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_epoch_secs_u64() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub fn now_epoch_secs_i64() -> i64 {
    i64::try_from(now_epoch_secs_u64()).unwrap_or(0)
}
