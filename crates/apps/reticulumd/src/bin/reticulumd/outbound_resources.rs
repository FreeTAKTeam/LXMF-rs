use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Correlation belongs to the retained native Resource owner. Receipt commits
/// and broadcast consumers cannot retire a still-active native transfer.
pub(super) type OutboundResourceMap = Arc<Mutex<HashMap<String, String>>>;

pub(super) fn track_owned_resource(
    map: &OutboundResourceMap,
    hash: String,
    message: String,
) -> std::io::Result<()> {
    let mut guard = map.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
    if guard.contains_key(&hash) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Resource already tracked",
        ));
    }
    guard.insert(hash, message);
    Ok(())
}

pub(super) fn retire_owned_resource(
    map: &OutboundResourceMap,
    hash: &str,
    message: &str,
) -> std::io::Result<()> {
    let mut guard = map.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
    if guard.get(hash).is_some_and(|owner| owner == message) {
        guard.remove(hash);
    }
    Ok(())
}
