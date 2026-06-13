use super::*;

const PN_STAMP_THROTTLE_SECS: i64 = 180;

const PR_REQUEST_SENT: u32 = 0x04;

const PR_COMPLETE: u32 = 0x07;

const PR_IDLE: u32 = 0x00;

const PR_NO_ACCESS: u32 = 0xf4;

const PR_FAILED: u32 = 0xfe;

fn is_remote_transfer_attempt_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::InvalidData
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::NotFound
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::WouldBlock
    )
}

fn remote_transfer_incomplete_error(
    result: &JsonValue,
    default_message: &str,
) -> Option<std::io::Error> {
    let postponed = result.get("postponed").and_then(JsonValue::as_bool).unwrap_or(false);
    let unsynced = result.get("synced").and_then(JsonValue::as_bool) == Some(false);
    if !postponed && !unsynced {
        return None;
    }

    let message = result
        .get("error")
        .and_then(JsonValue::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(default_message);
    Some(std::io::Error::new(std::io::ErrorKind::WouldBlock, message))
}

struct RemotePropagationImportSummary {
    imported_count: usize,
    duplicate_count: usize,
    imported_ids: Vec<String>,
    accepted_ids: Vec<String>,
    transferred_bytes: usize,
}
